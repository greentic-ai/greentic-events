use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use greentic_pack::events::EventProviderSpec;
use greentic_types::{ChannelMessageEnvelope, EventEnvelope, EventProviderDescriptor, TenantCtx};

use crate::bridge::{BridgeFactory, EventToMessageBridge, MessageToEventBridge};
use crate::provider::{EventProvider, EventProviderFactory, descriptor_from_spec};
use crate::subscription::{AckMode, SubscriptionHandle, SubscriptionOptions, subscription_channel};

type MessageBridgeBuilder =
    Arc<dyn Fn(&str) -> Result<Arc<dyn MessageToEventBridge>> + Send + Sync>;
type EventBridgeBuilder = Arc<dyn Fn(&str) -> Result<Arc<dyn EventToMessageBridge>> + Send + Sync>;
type ProviderBuilder =
    Arc<dyn Fn(&EventProviderSpec) -> Result<Arc<dyn EventProvider>> + Send + Sync>;

/// In-memory provider factory useful for tests/CLI without a runner.
pub struct InMemoryProviderFactory {
    builder: Arc<dyn Fn() -> Arc<dyn EventProvider> + Send + Sync>,
}

impl InMemoryProviderFactory {
    pub fn new(builder: impl Fn() -> Arc<dyn EventProvider> + Send + Sync + 'static) -> Self {
        Self {
            builder: Arc::new(builder),
        }
    }
}

#[async_trait::async_trait]
impl EventProviderFactory for InMemoryProviderFactory {
    async fn build_provider(
        &self,
        _spec: &EventProviderSpec,
    ) -> anyhow::Result<Arc<dyn EventProvider>> {
        let provider = (self.builder)();
        Ok(provider)
    }
}

/// Adapter that delegates bridge construction to runner-hosted invocations.
pub struct RunnerBridgeFactory {
    pub message_to_event: MessageBridgeBuilder,
    pub event_to_message: EventBridgeBuilder,
}

#[async_trait::async_trait]
impl BridgeFactory for RunnerBridgeFactory {
    async fn build_message_to_event(&self, name: &str) -> Result<Arc<dyn MessageToEventBridge>> {
        (self.message_to_event)(name)
    }

    async fn build_event_to_message(&self, name: &str) -> Result<Arc<dyn EventToMessageBridge>> {
        (self.event_to_message)(name)
    }
}

/// Adapter that delegates provider construction to runner-hosted invocations.
pub struct RunnerProviderFactory {
    pub build: ProviderBuilder,
}

#[async_trait::async_trait]
impl EventProviderFactory for RunnerProviderFactory {
    async fn build_provider(&self, spec: &EventProviderSpec) -> Result<Arc<dyn EventProvider>> {
        (self.build)(spec)
    }
}

/// Simplified WIT-aligned envelope used when talking to runner-hosted providers.
#[derive(Clone, Debug, PartialEq)]
pub struct WitEventEnvelope {
    pub id: String,
    pub topic: String,
    pub r#type: String,
    pub source: String,
    pub tenant: TenantCtx,
    pub subject: Option<String>,
    pub time: String,
    pub correlation_id: Option<String>,
    pub payload_json: String,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WitAckMode {
    Auto,
    Manual,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WitSubscriptionOptions {
    pub durable: bool,
    pub deliver_existing: bool,
    pub ack_mode: WitAckMode,
}

impl From<SubscriptionOptions> for WitSubscriptionOptions {
    fn from(value: SubscriptionOptions) -> Self {
        Self {
            durable: value.durable,
            deliver_existing: value.deliver_existing,
            ack_mode: match value.ack_mode {
                AckMode::Auto => WitAckMode::Auto,
                AckMode::Manual => WitAckMode::Manual,
            },
        }
    }
}

#[async_trait::async_trait]
pub trait RunnerBrokerClient: Send + Sync {
    async fn publish(&self, event: WitEventEnvelope) -> Result<()>;
    async fn subscribe(&self, topic: &str, opts: WitSubscriptionOptions) -> Result<String>;
    async fn next_event(
        &self,
        subscription: &str,
        timeout: Duration,
    ) -> Result<Option<WitEventEnvelope>>;
    async fn ack_event(&self, subscription: &str, event_id: &str) -> Result<()>;
}

/// Event provider backed by runner-hosted WIT broker components.
pub struct RunnerBrokerProvider {
    descriptor: EventProviderDescriptor,
    topics: Vec<String>,
    client: Arc<dyn RunnerBrokerClient>,
    poll_timeout: Duration,
}

impl RunnerBrokerProvider {
    pub fn new(
        descriptor: EventProviderDescriptor,
        topics: Vec<String>,
        client: Arc<dyn RunnerBrokerClient>,
    ) -> Self {
        Self {
            descriptor,
            topics,
            client,
            poll_timeout: Duration::from_millis(1_000),
        }
    }

    fn to_wit_event(event: &EventEnvelope) -> Result<WitEventEnvelope> {
        Ok(WitEventEnvelope {
            id: event.id.to_string(),
            topic: event.topic.clone(),
            r#type: event.r#type.clone(),
            source: event.source.clone(),
            tenant: event.tenant.clone(),
            subject: event.subject.clone(),
            time: event.time.to_rfc3339(),
            correlation_id: event.correlation_id.clone(),
            payload_json: serde_json::to_string(&event.payload)?,
            metadata: event.metadata.clone(),
        })
    }

    fn from_wit_event(wit: WitEventEnvelope) -> Result<EventEnvelope> {
        let time = DateTime::parse_from_rfc3339(&wit.time)
            .map_err(|err| anyhow::anyhow!(err))?
            .with_timezone(&Utc);
        Ok(EventEnvelope {
            id: greentic_types::EventId::new(wit.id)?,
            topic: wit.topic,
            r#type: wit.r#type,
            source: wit.source,
            tenant: wit.tenant,
            subject: wit.subject,
            time,
            correlation_id: wit.correlation_id,
            payload: serde_json::from_str(&wit.payload_json)?,
            metadata: wit.metadata,
        })
    }
}

#[async_trait::async_trait]
impl EventProvider for RunnerBrokerProvider {
    fn descriptor(&self) -> &EventProviderDescriptor {
        &self.descriptor
    }

    fn topic_patterns(&self) -> &[String] {
        &self.topics
    }

    async fn publish(&self, event: EventEnvelope) -> Result<()> {
        let wit = RunnerBrokerProvider::to_wit_event(&event)?;
        self.client.publish(wit).await
    }

    async fn subscribe(
        &self,
        topic: &str,
        _tenant: TenantCtx,
        options: SubscriptionOptions,
    ) -> Result<SubscriptionHandle> {
        let opts = WitSubscriptionOptions::from(options.clone());
        let subscription_id = self.client.subscribe(topic, opts).await?;
        let client = Arc::clone(&self.client);
        let (tx, handle) = subscription_channel();
        let timeout = self.poll_timeout;
        let subscription_name = subscription_id.clone();
        tokio::spawn(async move {
            loop {
                match client.next_event(&subscription_id, timeout).await {
                    Ok(Some(wit_event)) => match RunnerBrokerProvider::from_wit_event(wit_event) {
                        Ok(ev) => {
                            let ack = ev.id.to_string();
                            if tx.send(ev).await.is_err() {
                                break;
                            }
                            if options.ack_mode == AckMode::Auto {
                                let _ = client.ack_event(&subscription_name, &ack).await;
                            }
                        }
                        Err(_) => break,
                    },
                    Ok(None) => {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        continue;
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(handle)
    }
}

/// Factory that builds runner-backed broker providers directly from pack specs.
pub struct RunnerWitProviderFactory {
    client: Arc<dyn RunnerBrokerClient>,
}

impl RunnerWitProviderFactory {
    pub fn new(client: Arc<dyn RunnerBrokerClient>) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl EventProviderFactory for RunnerWitProviderFactory {
    async fn build_provider(&self, spec: &EventProviderSpec) -> Result<Arc<dyn EventProvider>> {
        let descriptor = descriptor_from_spec(spec);
        let topics = spec.capabilities.topics.clone();
        Ok(Arc::new(RunnerBrokerProvider::new(
            descriptor,
            topics,
            Arc::clone(&self.client),
        )))
    }
}

/// Helper bridge implementations backed by callables for tests/integration.
pub struct FnMessageToEventBridge {
    handler: Arc<dyn Fn(ChannelMessageEnvelope) -> Result<Vec<EventEnvelope>> + Send + Sync>,
}

impl FnMessageToEventBridge {
    pub fn new(
        handler: impl Fn(ChannelMessageEnvelope) -> Result<Vec<EventEnvelope>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            handler: Arc::new(handler),
        }
    }
}

#[async_trait::async_trait]
impl MessageToEventBridge for FnMessageToEventBridge {
    async fn handle_message(&self, message: ChannelMessageEnvelope) -> Result<Vec<EventEnvelope>> {
        (self.handler)(message)
    }
}

pub struct FnEventToMessageBridge {
    handler: Arc<dyn Fn(EventEnvelope) -> Result<Vec<ChannelMessageEnvelope>> + Send + Sync>,
}

impl FnEventToMessageBridge {
    pub fn new(
        handler: impl Fn(EventEnvelope) -> Result<Vec<ChannelMessageEnvelope>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            handler: Arc::new(handler),
        }
    }
}

#[async_trait::async_trait]
impl EventToMessageBridge for FnEventToMessageBridge {
    async fn handle_event(&self, event: EventEnvelope) -> Result<Vec<ChannelMessageEnvelope>> {
        (self.handler)(event)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WitAttachment {
    pub mime_type: String,
    pub url: String,
    pub name: Option<String>,
    pub size_bytes: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WitChannelMessageEnvelope {
    pub id: String,
    pub tenant: TenantCtx,
    pub channel: String,
    pub session_id: String,
    pub user_id: Option<String>,
    pub text: Option<String>,
    pub attachments: Vec<WitAttachment>,
    pub metadata: BTreeMap<String, String>,
}

#[async_trait::async_trait]
pub trait RunnerBridgeClient: Send + Sync {
    async fn handle_message(
        &self,
        name: &str,
        message: WitChannelMessageEnvelope,
    ) -> Result<Vec<WitEventEnvelope>>;

    async fn handle_event(
        &self,
        name: &str,
        event: WitEventEnvelope,
    ) -> Result<Vec<WitChannelMessageEnvelope>>;
}

fn to_wit_message(msg: &ChannelMessageEnvelope) -> WitChannelMessageEnvelope {
    WitChannelMessageEnvelope {
        id: msg.id.clone(),
        tenant: msg.tenant.clone(),
        channel: msg.channel.clone(),
        session_id: msg.session_id.clone(),
        user_id: msg.user_id.clone(),
        text: msg.text.clone(),
        attachments: msg
            .attachments
            .iter()
            .map(|a| WitAttachment {
                mime_type: a.mime_type.clone(),
                url: a.url.clone(),
                name: a.name.clone(),
                size_bytes: a.size_bytes,
            })
            .collect(),
        metadata: msg.metadata.clone(),
    }
}

fn from_wit_message(msg: WitChannelMessageEnvelope) -> ChannelMessageEnvelope {
    ChannelMessageEnvelope {
        id: msg.id,
        tenant: msg.tenant,
        channel: msg.channel,
        session_id: msg.session_id,
        reply_scope: None,
        user_id: msg.user_id,
        correlation_id: None,
        text: msg.text,
        attachments: msg
            .attachments
            .into_iter()
            .map(|a| greentic_types::Attachment {
                mime_type: a.mime_type,
                url: a.url,
                name: a.name,
                size_bytes: a.size_bytes,
            })
            .collect(),
        metadata: msg.metadata,
    }
}

/// Bridge backed by runner-hosted WIT message->event component.
pub struct RunnerMessageToEventBridgeAdapter {
    name: String,
    client: Arc<dyn RunnerBridgeClient>,
}

impl RunnerMessageToEventBridgeAdapter {
    pub fn new(name: impl Into<String>, client: Arc<dyn RunnerBridgeClient>) -> Self {
        Self {
            name: name.into(),
            client,
        }
    }
}

#[async_trait::async_trait]
impl MessageToEventBridge for RunnerMessageToEventBridgeAdapter {
    async fn handle_message(&self, message: ChannelMessageEnvelope) -> Result<Vec<EventEnvelope>> {
        let wit_msg = to_wit_message(&message);
        let events = self
            .client
            .handle_message(&self.name, wit_msg)
            .await?
            .into_iter()
            .map(RunnerBrokerProvider::from_wit_event)
            .collect::<Result<Vec<_>>>()?;
        Ok(events)
    }
}

/// Bridge backed by runner-hosted WIT event->message component.
pub struct RunnerEventToMessageBridgeAdapter {
    name: String,
    client: Arc<dyn RunnerBridgeClient>,
}

impl RunnerEventToMessageBridgeAdapter {
    pub fn new(name: impl Into<String>, client: Arc<dyn RunnerBridgeClient>) -> Self {
        Self {
            name: name.into(),
            client,
        }
    }
}

#[async_trait::async_trait]
impl EventToMessageBridge for RunnerEventToMessageBridgeAdapter {
    async fn handle_event(&self, event: EventEnvelope) -> Result<Vec<ChannelMessageEnvelope>> {
        let wit_event = RunnerBrokerProvider::to_wit_event(&event)?;
        let messages = self
            .client
            .handle_event(&self.name, wit_event)
            .await?
            .into_iter()
            .map(from_wit_message)
            .collect();
        Ok(messages)
    }
}

/// Bridge factory that instantiates runner-backed bridge adapters.
pub struct RunnerWitBridgeFactory {
    client: Arc<dyn RunnerBridgeClient>,
}

impl RunnerWitBridgeFactory {
    pub fn new(client: Arc<dyn RunnerBridgeClient>) -> Self {
        Self { client }
    }
}

#[async_trait::async_trait]
impl BridgeFactory for RunnerWitBridgeFactory {
    async fn build_message_to_event(&self, name: &str) -> Result<Arc<dyn MessageToEventBridge>> {
        Ok(Arc::new(RunnerMessageToEventBridgeAdapter::new(
            name,
            Arc::clone(&self.client),
        )))
    }

    async fn build_event_to_message(&self, name: &str) -> Result<Arc<dyn EventToMessageBridge>> {
        Ok(Arc::new(RunnerEventToMessageBridgeAdapter::new(
            name,
            Arc::clone(&self.client),
        )))
    }
}

/// Minimal no-op provider that accepts publishes and drops them, with no subscriptions.
pub struct NoopProvider {
    descriptor: greentic_types::EventProviderDescriptor,
}

impl NoopProvider {
    pub fn new() -> Self {
        Self {
            descriptor: greentic_types::EventProviderDescriptor {
                name: "noop".into(),
                kind: greentic_types::EventProviderKind::Broker,
                transport: greentic_types::TransportKind::Other("noop".into()),
                reliability: greentic_types::ReliabilityKind::AtMostOnce,
                ordering: greentic_types::OrderingKind::None,
                notes: Some("In-memory noop provider".into()),
                tags: vec!["noop".into()],
            },
        }
    }
}

impl Default for NoopProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl EventProvider for NoopProvider {
    fn descriptor(&self) -> &greentic_types::EventProviderDescriptor {
        &self.descriptor
    }

    fn topic_patterns(&self) -> &[String] {
        &[]
    }

    async fn publish(&self, _event: EventEnvelope) -> anyhow::Result<()> {
        Ok(())
    }

    async fn subscribe(
        &self,
        _topic: &str,
        _tenant: greentic_types::TenantCtx,
        _options: crate::subscription::SubscriptionOptions,
    ) -> anyhow::Result<crate::subscription::SubscriptionHandle> {
        Err(anyhow::anyhow!("Noop provider does not support subscribe"))
    }
}
