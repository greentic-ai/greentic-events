use std::collections::BTreeMap;

use greentic_types::{EventEnvelope as TypesEventEnvelope, TenantCtx};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Re-export of the canonical event envelope shared with greentic-types.
pub type EventEnvelope = TypesEventEnvelope;

/// Input contract for the provider-core `publish` operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublishInput {
    pub topic: String,
    pub event: EventEnvelopeInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<PublishOptions>,
}

/// Event envelope accepted by provider-core publish.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventEnvelopeInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    pub payload: Value,
    #[serde(default, skip_serializing_if = "EventMeta::is_empty")]
    pub meta: EventMeta,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<TenantCtx>,
}

/// Lightweight metadata wrapper for publish events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct EventMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(flatten, default)]
    pub extra: BTreeMap<String, Value>,
}

impl EventMeta {
    pub fn is_empty(&self) -> bool {
        self.tenant.is_none() && self.tags.is_empty() && self.extra.is_empty()
    }
}

/// Publish-time options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublishOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<PublishPriority>,
}

/// Priority hint for providers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PublishPriority {
    Low,
    Normal,
    High,
}

/// Output contract for the provider-core `publish` operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublishOutput {
    pub receipt_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_event_id: Option<String>,
    pub status: PublishStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
}

/// Status returned by a provider for publish operations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PublishStatus {
    Published,
    Queued,
}
