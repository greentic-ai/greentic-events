use std::sync::Arc;

use anyhow::Result;
use greentic_pack::events::EventProviderSpec;
use greentic_types::{ChannelMessageEnvelope, EventEnvelope};

use crate::bridge::{BridgeFactory, EventToMessageBridge, MessageToEventBridge};
use crate::provider::{EventProvider, EventProviderFactory};

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
