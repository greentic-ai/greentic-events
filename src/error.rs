use thiserror::Error;

/// Unified result type for the event bus.
pub type Result<T> = std::result::Result<T, EventBusError>;

/// Errors emitted by the event bus and related services.
#[derive(Debug, Error)]
pub enum EventBusError {
    /// No provider matched the requested topic.
    #[error("no provider registered for topic {topic}")]
    NoProvider { topic: String },

    /// An access control rule denied the operation.
    #[error("access denied for tenant {tenant} on topic {topic} ({operation})")]
    AclDenied {
        tenant: String,
        topic: String,
        operation: &'static str,
    },

    /// An idempotent publish was skipped because the key was already seen.
    #[error("duplicate event skipped due to idempotency key {key}")]
    Duplicate { key: String },

    /// Provider failed after exhausting retries.
    #[error("provider {provider} failed after retries: {reason}")]
    DeliveryFailed { provider: String, reason: String },

    /// A bridge with the given name could not be found.
    #[error("bridge {bridge} not found")]
    BridgeNotFound { bridge: String },

    /// Bridge invocation failed.
    #[error("bridge error: {0}")]
    BridgeError(String),

    /// Catch-all for lower-level errors.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
