use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use greentic_pack::events::EventProviderSpec;
use greentic_types::{EventEnvelope, EventProviderDescriptor, TenantCtx};

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
