#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use greentic_events::subscription::{
    SubscriptionHandle, SubscriptionOptions, subscription_channel,
};
use greentic_events::{EventProvider, EventProviderFactory, ProviderRegistration};
use greentic_pack::events::EventProviderSpec;
use greentic_types::{
    Attachment, ChannelMessageEnvelope, EventEnvelope, EventId, EventProviderDescriptor,
    EventProviderKind, OrderingKind, ReliabilityKind, TenantCtx, TransportKind,
};
use serde_json::json;
use tokio::sync::broadcast;

use greentic_events::backoff::RetryPolicy;
use greentic_events::bridge::{EventToMessageBridge, MessageToEventBridge};

pub fn tenant_ctx() -> TenantCtx {
    TenantCtx::new("dev".try_into().unwrap(), "tenant-a".try_into().unwrap())
}

#[derive(Clone)]
pub struct FakeProvider {
    descriptor: EventProviderDescriptor,
    topics: Vec<String>,
    sender: broadcast::Sender<EventEnvelope>,
    published: Arc<Mutex<Vec<EventEnvelope>>>,
    fail_first: Arc<AtomicU32>,
    fail_reason: Arc<Option<String>>,
}

impl FakeProvider {
    pub fn new(name: &str, topics: Vec<String>, fail_first: u32) -> Self {
        let (sender, _) = broadcast::channel(64);
        Self {
            descriptor: EventProviderDescriptor {
                name: name.to_string(),
                kind: EventProviderKind::Broker,
                transport: TransportKind::Nats,
                reliability: ReliabilityKind::AtLeastOnce,
                ordering: OrderingKind::PerKey,
                notes: None,
                tags: vec![],
            },
            topics,
            sender,
            published: Arc::new(Mutex::new(Vec::new())),
            fail_first: Arc::new(AtomicU32::new(fail_first)),
            fail_reason: Arc::new(None),
        }
    }

    pub fn published(&self) -> Arc<Mutex<Vec<EventEnvelope>>> {
        Arc::clone(&self.published)
    }

    pub fn with_fail_reason(mut self, reason: impl Into<String>) -> Self {
        self.fail_reason = Arc::new(Some(reason.into()));
        self
    }

    pub fn remaining_failures(&self) -> u32 {
        self.fail_first.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl EventProvider for FakeProvider {
    fn descriptor(&self) -> &EventProviderDescriptor {
        &self.descriptor
    }

    fn topic_patterns(&self) -> &[String] {
        &self.topics
    }

    async fn publish(&self, event: EventEnvelope) -> Result<()> {
        if !event.topic.ends_with(".dlq")
            && self
                .fail_first
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    if remaining > 0 {
                        Some(remaining - 1)
                    } else {
                        None
                    }
                })
                .is_ok()
        {
            let reason = self
                .fail_reason
                .as_ref()
                .clone()
                .unwrap_or_else(|| "forced publish failure".into());
            anyhow::bail!(reason);
        }
        self.published.lock().unwrap().push(event.clone());
        let _ = self.sender.send(event);
        Ok(())
    }

    async fn subscribe(
        &self,
        _topic: &str,
        _tenant: TenantCtx,
        options: SubscriptionOptions,
    ) -> Result<SubscriptionHandle> {
        let mut rx = self.sender.subscribe();
        let (tx, handle) = subscription_channel();
        if options.deliver_existing {
            let existing: Vec<_> = self.published.lock().unwrap().clone();
            for ev in existing {
                tx.send(ev).await.expect("send existing");
            }
        }
        tokio::spawn(async move {
            while let Ok(ev) = rx.recv().await {
                if tx.send(ev).await.is_err() {
                    break;
                }
            }
        });
        Ok(handle)
    }
}

#[derive(Default)]
pub struct FakeProviderFactory {
    pub fail_first: HashMap<String, u32>,
    pub providers: Arc<Mutex<HashMap<String, Arc<FakeProvider>>>>,
}

#[async_trait]
impl EventProviderFactory for FakeProviderFactory {
    async fn build_provider(&self, spec: &EventProviderSpec) -> Result<Arc<dyn EventProvider>> {
        let fail = self.fail_first.get(&spec.name).copied().unwrap_or(0);
        let provider = Arc::new(FakeProvider::new(
            &spec.name,
            spec.capabilities.topics.clone(),
            fail,
        ));
        self.providers
            .lock()
            .unwrap()
            .insert(spec.name.clone(), Arc::clone(&provider));
        Ok(provider)
    }
}

pub fn registration_from_provider(
    provider: Arc<FakeProvider>,
    retry: RetryPolicy,
    dlq: Option<String>,
) -> ProviderRegistration {
    ProviderRegistration {
        descriptor: provider.descriptor().clone(),
        topics: provider.topic_patterns().to_vec(),
        dlq_topic: dlq,
        retry,
        instance: provider,
    }
}

pub struct FakeMessageToEventBridge;

#[async_trait]
impl MessageToEventBridge for FakeMessageToEventBridge {
    async fn handle_message(&self, message: ChannelMessageEnvelope) -> Result<Vec<EventEnvelope>> {
        let ev = EventEnvelope {
            id: EventId::new(format!("evt-{}", message.id)).unwrap(),
            topic: format!("bridge.{}", message.channel),
            r#type: "bridge.converted".into(),
            source: "bridge".into(),
            tenant: message.tenant.clone(),
            subject: Some(message.session_id.clone()),
            time: Utc::now(),
            correlation_id: None,
            payload: json!({"text": message.text}),
            metadata: message.metadata.clone(),
        };
        Ok(vec![ev])
    }
}

pub struct FakeEventToMessageBridge;

#[async_trait]
impl EventToMessageBridge for FakeEventToMessageBridge {
    async fn handle_event(&self, event: EventEnvelope) -> Result<Vec<ChannelMessageEnvelope>> {
        let msg = ChannelMessageEnvelope {
            id: format!("msg-{}", event.id),
            tenant: event.tenant.clone(),
            channel: event.topic.clone(),
            session_id: event.subject.clone().unwrap_or_else(|| "session".into()),
            user_id: None,
            text: Some(event.payload.to_string()),
            attachments: vec![Attachment {
                mime_type: "application/json".into(),
                url: "https://example.invalid/payload".into(),
                name: Some("payload.json".into()),
                size_bytes: None,
            }],
            metadata: event.metadata.clone(),
        };
        Ok(vec![msg])
    }
}
