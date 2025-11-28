use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use greentic_pack::events::EventProviderSpec;
use greentic_types::TransportKind;
use greentic_types::{EventEnvelope, EventProviderDescriptor, TenantCtx};
use greentic_types::{EventProviderKind as TypesEventProviderKind, OrderingKind, ReliabilityKind};

use crate::backoff::RetryPolicy;
use crate::subscription::{SubscriptionHandle, SubscriptionOptions};

/// Provider implementation that can publish and subscribe to events.
#[async_trait]
pub trait EventProvider: Send + Sync {
    fn descriptor(&self) -> &EventProviderDescriptor;
    fn topic_patterns(&self) -> &[String];

    async fn publish(&self, event: EventEnvelope) -> Result<()>;

    async fn subscribe(
        &self,
        topic: &str,
        tenant: TenantCtx,
        options: SubscriptionOptions,
    ) -> Result<SubscriptionHandle>;
}

/// Factory used to instantiate providers from pack specs.
#[async_trait]
pub trait EventProviderFactory: Send + Sync {
    async fn build_provider(&self, spec: &EventProviderSpec) -> Result<Arc<dyn EventProvider>>;
}

/// Runtime configuration for a provider after registration.
#[derive(Clone)]
pub struct ProviderRegistration {
    pub descriptor: EventProviderDescriptor,
    pub topics: Vec<String>,
    pub dlq_topic: Option<String>,
    pub retry: RetryPolicy,
    pub instance: Arc<dyn EventProvider>,
}

/// Build a provider descriptor from a pack spec using shared mappings.
pub fn descriptor_from_spec(spec: &EventProviderSpec) -> EventProviderDescriptor {
    EventProviderDescriptor {
        name: spec.name.clone(),
        kind: match spec.kind {
            greentic_pack::events::EventProviderKind::Broker => TypesEventProviderKind::Broker,
            greentic_pack::events::EventProviderKind::Source => TypesEventProviderKind::Source,
            greentic_pack::events::EventProviderKind::Sink => TypesEventProviderKind::Sink,
            greentic_pack::events::EventProviderKind::Bridge => TypesEventProviderKind::Bridge,
        },
        transport: spec
            .capabilities
            .transport
            .as_ref()
            .map(cap_transport_to_types)
            .unwrap_or(TransportKind::Other("unspecified".into())),
        reliability: spec
            .capabilities
            .reliability
            .as_ref()
            .map(cap_reliability_to_types)
            .unwrap_or(ReliabilityKind::AtMostOnce),
        ordering: spec
            .capabilities
            .ordering
            .as_ref()
            .map(cap_ordering_to_types)
            .unwrap_or(OrderingKind::None),
        notes: None,
        tags: vec![format!("{:?}", spec.kind)],
    }
}

fn cap_transport_to_types(value: &greentic_pack::events::TransportKind) -> TransportKind {
    match value {
        greentic_pack::events::TransportKind::Nats => TransportKind::Nats,
        greentic_pack::events::TransportKind::Kafka => TransportKind::Kafka,
        greentic_pack::events::TransportKind::Sqs => TransportKind::Sqs,
        greentic_pack::events::TransportKind::Webhook => TransportKind::Webhook,
        greentic_pack::events::TransportKind::Email => TransportKind::Email,
        greentic_pack::events::TransportKind::Other(v) => match v.to_ascii_lowercase().as_str() {
            "nats" => TransportKind::Nats,
            "kafka" => TransportKind::Kafka,
            "sqs" => TransportKind::Sqs,
            "webhook" => TransportKind::Webhook,
            "email" => TransportKind::Email,
            other => TransportKind::Other(other.to_string()),
        },
    }
}

fn cap_reliability_to_types(value: &greentic_pack::events::ReliabilityKind) -> ReliabilityKind {
    match value {
        greentic_pack::events::ReliabilityKind::AtMostOnce => ReliabilityKind::AtMostOnce,
        greentic_pack::events::ReliabilityKind::AtLeastOnce => ReliabilityKind::AtLeastOnce,
        greentic_pack::events::ReliabilityKind::EffectivelyOnce => ReliabilityKind::EffectivelyOnce,
    }
}

fn cap_ordering_to_types(value: &greentic_pack::events::OrderingKind) -> OrderingKind {
    match value {
        greentic_pack::events::OrderingKind::None => OrderingKind::None,
        greentic_pack::events::OrderingKind::PerKey => OrderingKind::PerKey,
        greentic_pack::events::OrderingKind::Global => OrderingKind::Global,
    }
}
