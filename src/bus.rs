use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use chrono::Utc;
use greentic_pack::events::EventProviderKind as PackEventProviderKind;
use greentic_pack::events::EventsSection;
use greentic_types::{EventEnvelope, EventId, EventProviderDescriptor, TenantCtx};
use rand::Rng;
use rand::distr::Alphanumeric;
use serde::Deserialize;
use serde_json::Value;
use tokio::time::sleep;
use tracing::{Instrument, info_span};

use crate::acl::{TopicAcl, TopicAclRule};
use crate::backoff::RetryPolicy;
use crate::bridge::{BridgeRegistry, BridgeService, EventToMessageBridge, MessageToEventBridge};
use crate::error::{EventBusError, Result};
use crate::idempotency::{IdempotencyStore, InMemoryIdempotencyStore};
use crate::metrics::EventMetrics;
use crate::pattern::matches_pattern;
use crate::provider::{EventProviderFactory, ProviderRegistration, descriptor_from_spec};
use crate::subscription::{SubscriptionHandle, SubscriptionOptions};

#[derive(Debug, Deserialize)]
struct PackSpec {
    #[serde(rename = "packVersion", default = "default_pack_version")]
    pack_version: u32,
    id: String,
    version: String,
    #[serde(default)]
    annotations: serde_json::Map<String, Value>,
    #[serde(default)]
    events: Option<EventsSection>,
}

#[derive(Debug)]
struct SpecBundle {
    spec: PackSpec,
    #[allow(dead_code)]
    source: std::path::PathBuf,
}

fn default_pack_version() -> u32 {
    1
}

/// High-level event bus coordinating provider routing, reliability, telemetry, and ACLs.
pub struct EventBus {
    providers: Vec<ProviderRegistration>,
    acl: TopicAcl,
    idempotency: Arc<dyn IdempotencyStore>,
    metrics: EventMetrics,
    bridges: BridgeService,
}

impl EventBus {
    /// Publish an event through the appropriate provider with retries, DLQ, and idempotency.
    pub async fn publish(&self, event: EventEnvelope) -> Result<()> {
        self.acl.allow_publish(&event.tenant, &event.topic)?;
        let provider = self.provider_for_topic(&event.topic)?;
        let span = info_span!(
            "event.publish",
            tenant_id = %event.tenant.tenant,
            topic = %event.topic,
            provider = %provider.descriptor.name,
            dlq = false
        );
        async {
            self.metrics.record_in();
            if let Some(key) = event.metadata.get("idempotency_key") {
                let composed = format!("{}:{}", event.tenant.tenant, key);
                if !self.idempotency.record_key(&composed) {
                    return Err(EventBusError::Duplicate { key: composed });
                }
            }

            let start = Instant::now();
            match self.publish_with_retry(provider, event.clone()).await {
                Ok(_) => {
                    self.metrics.record_out(start.elapsed());
                    Ok(())
                }
                Err(err) => {
                    self.metrics.record_failed();
                    let dlq_topic = provider
                        .dlq_topic
                        .clone()
                        .unwrap_or_else(|| format!("{}.dlq", event.topic));
                    let mut dlq_event = event.clone();
                    dlq_event.topic = dlq_topic.clone();
                    dlq_event
                        .metadata
                        .insert("dlq_reason".into(), err.to_string());
                    self.publish_to_dlq(provider, dlq_event).await;
                    Err(EventBusError::DeliveryFailed {
                        provider: provider.descriptor.name.clone(),
                        reason: err.to_string(),
                    })
                }
            }
        }
        .instrument(span)
        .await
    }

    /// Convenience helper to publish a JSON payload with a generated envelope.
    pub async fn publish_event(
        &self,
        tenant: &TenantCtx,
        topic: &str,
        payload: Value,
    ) -> Result<()> {
        let envelope = build_envelope(tenant.clone(), topic, payload)?;
        self.publish(envelope).await
    }

    /// Subscribe to a topic using the provider mapped to that topic.
    pub async fn subscribe(
        &self,
        topic: &str,
        tenant: TenantCtx,
        options: SubscriptionOptions,
    ) -> Result<SubscriptionHandle> {
        self.acl.allow_subscribe(&tenant, topic)?;
        let provider = self.provider_for_topic(topic)?;
        let span = info_span!(
            "event.subscribe",
            tenant_id = %tenant.tenant,
            topic,
            provider = %provider.descriptor.name
        );
        provider
            .instance
            .subscribe(topic, tenant, options)
            .instrument(span)
            .await
            .map_err(EventBusError::Other)
    }

    /// Convenience helper to subscribe with a borrowed tenant context.
    pub async fn subscribe_topic(
        &self,
        topic: &str,
        tenant: &TenantCtx,
        options: SubscriptionOptions,
    ) -> Result<SubscriptionHandle> {
        self.subscribe(topic, tenant.clone(), options).await
    }

    /// Returns registered provider descriptors for introspection.
    pub fn list_providers(&self) -> Vec<EventProviderDescriptor> {
        self.providers
            .iter()
            .map(|p| p.descriptor.clone())
            .collect()
    }

    /// Access to registered bridge service.
    pub fn bridges(&self) -> &BridgeService {
        &self.bridges
    }

    /// Access event metrics counters for telemetry export or testing.
    pub fn metrics(&self) -> &EventMetrics {
        &self.metrics
    }

    async fn publish_with_retry(
        &self,
        provider: &ProviderRegistration,
        event: EventEnvelope,
    ) -> anyhow::Result<()> {
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..=provider.retry.max_retries {
            let result = provider.instance.publish(event.clone()).await;
            match result {
                Ok(_) => return Ok(()),
                Err(err) => {
                    let err_str = err.to_string();
                    last_err = Some(anyhow::anyhow!(err_str.clone()));
                    let retryable = provider.retry.is_retryable(&err_str);
                    if attempt == provider.retry.max_retries || !retryable {
                        break;
                    }
                    let delay = provider.retry.delay_for_attempt(attempt + 1);
                    sleep(delay).await;
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("unknown error")))
    }

    async fn publish_to_dlq(&self, provider: &ProviderRegistration, event: EventEnvelope) {
        let span = info_span!(
            "event.dlq",
            tenant_id = %event.tenant.tenant,
            topic = %event.topic,
            provider = %provider.descriptor.name,
            dlq = true
        );
        let _ = async {
            let _ = provider.instance.publish(event.clone()).await;
            self.metrics.record_dlq();
        }
        .instrument(span)
        .await;
    }

    fn provider_for_topic(&self, topic: &str) -> Result<&ProviderRegistration> {
        self.providers
            .iter()
            .find(|provider| {
                provider
                    .topics
                    .iter()
                    .any(|pattern| matches_pattern(pattern, topic))
            })
            .ok_or_else(|| EventBusError::NoProvider {
                topic: topic.to_string(),
            })
    }
}

/// Builder for configuring an event bus instance.
pub struct EventBusBuilder {
    providers: Vec<ProviderRegistration>,
    acl: TopicAcl,
    idempotency: Arc<dyn IdempotencyStore>,
    metrics: EventMetrics,
    bridges: BridgeRegistry,
    overrides: HashMap<String, ProviderOverrides>,
}

impl Default for EventBusBuilder {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            acl: TopicAcl {
                rules: Vec::new(),
                default_allow: true,
            },
            idempotency: Arc::new(InMemoryIdempotencyStore::new()),
            metrics: EventMetrics::new(),
            bridges: BridgeRegistry::new(),
            overrides: HashMap::new(),
        }
    }
}

impl EventBusBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_acl(mut self, rules: Vec<TopicAclRule>) -> Self {
        self.acl.rules = rules;
        self
    }

    pub fn with_idempotency_store(mut self, store: Arc<dyn IdempotencyStore>) -> Self {
        self.idempotency = store;
        self
    }

    pub fn with_metrics(mut self, metrics: EventMetrics) -> Self {
        self.metrics = metrics;
        self
    }

    pub fn with_overrides(mut self, overrides: HashMap<String, ProviderOverrides>) -> Self {
        self.overrides = overrides;
        self
    }

    pub fn register_provider(mut self, provider: ProviderRegistration) -> Self {
        self.providers.push(provider);
        self
    }

    pub fn register_message_to_event_bridge(
        mut self,
        name: impl Into<String>,
        bridge: Arc<dyn MessageToEventBridge>,
    ) -> Self {
        self.bridges.register_message_to_event(name, bridge);
        self
    }

    pub fn register_event_to_message_bridge(
        mut self,
        name: impl Into<String>,
        bridge: Arc<dyn EventToMessageBridge>,
    ) -> Self {
        self.bridges.register_event_to_message(name, bridge);
        self
    }

    pub async fn register_from_pack_dir(
        mut self,
        pack_dir: &Path,
        provider_factory: &dyn EventProviderFactory,
        bridge_factory: Option<&dyn crate::bridge::BridgeFactory>,
    ) -> anyhow::Result<Self> {
        let bundle = load_spec(pack_dir).context("failed to load pack spec")?;
        if let Some(events) = bundle.spec.events {
            for provider in events.providers {
                if let Some(ov) =
                    provider_overrides_from_annotations(&provider.name, &bundle.spec.annotations)
                {
                    self.overrides.entry(provider.name.clone()).or_insert(ov);
                }
                match provider.kind {
                    PackEventProviderKind::Bridge => {
                        if let Some(factory) = bridge_factory {
                            let msg_bridge = factory
                                .build_message_to_event(&provider.name)
                                .await
                                .context("build message->event bridge")?;
                            self.bridges
                                .register_message_to_event(&provider.name, msg_bridge);
                            let evt_bridge = factory
                                .build_event_to_message(&provider.name)
                                .await
                                .context("build event->message bridge")?;
                            self.bridges
                                .register_event_to_message(&provider.name, evt_bridge);
                        }
                    }
                    _ => {
                        let descriptor = descriptor_from_spec(&provider);
                        let topics = provider.capabilities.topics.clone();
                        let instance = provider_factory.build_provider(&provider).await?;
                        let override_cfg = self.overrides.get(&provider.name);
                        self.providers.push(ProviderRegistration {
                            descriptor,
                            topics,
                            dlq_topic: override_cfg.and_then(|o| o.dlq_topic.clone()),
                            retry: override_cfg
                                .and_then(|o| o.retry.clone())
                                .unwrap_or_default(),
                            instance,
                        });
                    }
                }
            }
        }
        Ok(self)
    }

    pub fn build(self) -> EventBus {
        let providers = self
            .providers
            .into_iter()
            .map(|mut reg| {
                if let Some(over) = self.overrides.get(&reg.descriptor.name) {
                    if let Some(dlq) = &over.dlq_topic {
                        reg.dlq_topic = Some(dlq.clone());
                    }
                    if let Some(retry) = &over.retry {
                        reg.retry = retry.clone();
                    }
                }
                reg
            })
            .collect();
        EventBus {
            providers,
            acl: self.acl,
            idempotency: self.idempotency,
            metrics: self.metrics,
            bridges: BridgeService::new(self.bridges),
        }
    }
}

/// Overrides applied per provider name when registering from packs.
#[derive(Clone, Debug, Default)]
pub struct ProviderOverrides {
    pub retry: Option<RetryPolicy>,
    pub dlq_topic: Option<String>,
}

fn load_spec(pack_dir: &Path) -> anyhow::Result<SpecBundle> {
    let manifest_path = pack_dir.join("pack.yaml");
    let contents = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let spec: PackSpec = serde_yaml_bw::from_str(&contents)
        .with_context(|| format!("{} is not a valid PackSpec", manifest_path.display()))?;
    validate_spec(&spec)?;
    if let Some(events) = &spec.events {
        events.validate()?;
    }
    Ok(SpecBundle {
        spec,
        source: manifest_path,
    })
}

fn validate_spec(spec: &PackSpec) -> anyhow::Result<()> {
    if spec.pack_version != 1 {
        anyhow::bail!("unsupported packVersion {}; expected 1", spec.pack_version);
    }
    if spec.id.trim().is_empty() {
        anyhow::bail!("pack id must not be empty");
    }
    if spec.version.trim().is_empty() {
        anyhow::bail!("pack version must not be empty");
    }
    Ok(())
}

/// Build a generic event envelope with a generated ID and default metadata.
pub fn build_envelope(
    tenant: TenantCtx,
    topic: impl Into<String>,
    payload: Value,
) -> Result<EventEnvelope> {
    Ok(EventEnvelope {
        id: generate_event_id()?,
        topic: topic.into(),
        r#type: "greentic.events.generic".into(),
        source: "greentic-events".into(),
        tenant,
        subject: None,
        time: Utc::now(),
        correlation_id: None,
        payload,
        metadata: Default::default(),
    })
}

fn provider_overrides_from_annotations(
    name: &str,
    annotations: &serde_json::Map<String, Value>,
) -> Option<ProviderOverrides> {
    let prefix = format!("events.providers.{name}.");
    let mut retry: Option<RetryPolicy> = None;
    let mut dlq_topic: Option<String> = None;

    for (key, value) in annotations {
        if !key.starts_with(&prefix) {
            continue;
        }
        let suffix = &key[prefix.len()..];
        match suffix {
            "dlq_topic" => {
                if let Some(s) = value.as_str() {
                    dlq_topic = Some(s.to_string());
                }
            }
            "retry.max_retries" => {
                if let Some(v) = value.as_u64() {
                    retry.get_or_insert_with(RetryPolicy::default).max_retries = v as u32;
                }
            }
            "retry.base_delay_ms" => {
                if let Some(v) = value.as_u64() {
                    retry.get_or_insert_with(RetryPolicy::default).base_delay =
                        std::time::Duration::from_millis(v);
                }
            }
            "retry.max_delay_ms" => {
                if let Some(v) = value.as_u64() {
                    retry.get_or_insert_with(RetryPolicy::default).max_delay =
                        std::time::Duration::from_millis(v);
                }
            }
            "retry.strategy" => {
                if let Some(s) = value.as_str() {
                    let strategy = match s.to_ascii_lowercase().as_str() {
                        "fixed" => crate::backoff::BackoffStrategy::Fixed,
                        "exponential" => crate::backoff::BackoffStrategy::Exponential,
                        _ => crate::backoff::BackoffStrategy::ExponentialWithJitter,
                    };
                    retry.get_or_insert_with(RetryPolicy::default).strategy = strategy;
                }
            }
            "retry.retryable_errors" => {
                if let Some(arr) = value.as_array() {
                    let mut errors = Vec::new();
                    for v in arr {
                        if let Some(s) = v.as_str() {
                            errors.push(s.to_string());
                        }
                    }
                    retry
                        .get_or_insert_with(RetryPolicy::default)
                        .retryable_errors = errors;
                }
            }
            _ => {}
        }
    }

    if retry.is_some() || dlq_topic.is_some() {
        Some(ProviderOverrides { retry, dlq_topic })
    } else {
        None
    }
}

fn generate_event_id() -> Result<EventId> {
    let rng = rand::rng();
    let suffix: String = rng
        .sample_iter(Alphanumeric)
        .take(12)
        .map(char::from)
        .collect();
    let id = format!("evt-{}", suffix.to_lowercase());
    EventId::new(id).map_err(|err| EventBusError::Other(err.into()))
}
