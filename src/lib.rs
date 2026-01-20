#![forbid(unsafe_code)]

pub mod acl;
pub mod backoff;
pub mod bridge;
pub mod bus;
pub mod e2e;
pub mod error;
pub mod idempotency;
pub mod metrics;
pub mod pattern;
pub mod provider;
pub mod provider_ops;
pub mod runner;
pub mod subscription;
pub mod validate;

pub use bus::{EventBus, EventBusBuilder, build_envelope};
pub use provider::{EventProvider, EventProviderFactory, ProviderRegistration};
pub use provider_ops::{
    EventEnvelope, EventEnvelopeInput, EventMeta, PublishInput, PublishOptions, PublishOutput,
    PublishPriority, PublishStatus,
};
pub use subscription::SubscriptionHandle;
pub use subscription::{AckMode, SubscriptionOptions};
pub use validate::{ValidationIssue, validate_publish_input};
