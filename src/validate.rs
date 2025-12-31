use crate::provider_ops::PublishInput;
use chrono::DateTime;

/// Aggregated validation issues encountered while checking an input payload.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationIssue {
    pub messages: Vec<String>,
}

impl ValidationIssue {
    pub fn new(messages: Vec<String>) -> Self {
        Self { messages }
    }

    pub fn single(message: impl Into<String>) -> Self {
        Self {
            messages: vec![message.into()],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

impl std::fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.messages.is_empty() {
            write!(f, "no validation issues")
        } else {
            write!(f, "{}", self.messages.join("; "))
        }
    }
}

impl std::error::Error for ValidationIssue {}

/// Validate a provider-core publish input payload.
pub fn validate_publish_input(input: &PublishInput) -> Result<(), ValidationIssue> {
    let mut errors = Vec::new();

    if input.topic.trim().is_empty() {
        errors.push("topic must not be empty".into());
    }

    if let Some(event_topic) = input.event.topic.as_deref() {
        if event_topic.trim().is_empty() {
            errors.push("event.topic must not be empty when provided".into());
        } else if event_topic != input.topic {
            errors.push("event.topic must match the top-level topic when provided".into());
        }
    }

    if let Some(id) = input.event.id.as_deref()
        && id.trim().is_empty()
    {
        errors.push("event.id must not be empty when provided".into());
    }

    if let Some(event_type) = input.event.r#type.as_deref()
        && event_type.trim().is_empty()
    {
        errors.push("event.type must not be empty when provided".into());
    }

    if let Some(source) = input.event.source.as_deref()
        && source.trim().is_empty()
    {
        errors.push("event.source must not be empty when provided".into());
    }

    if let Some(subject) = input.event.subject.as_deref()
        && subject.trim().is_empty()
    {
        errors.push("event.subject must not be empty when provided".into());
    }

    if let Some(ts) = input.event.ts.as_deref()
        && !is_rfc3339(ts)
    {
        errors.push("event.ts must be an RFC3339 timestamp".into());
    }

    if let Some(time) = input.event.time.as_deref()
        && !is_rfc3339(time)
    {
        errors.push("event.time must be an RFC3339 timestamp".into());
    }

    if let (Some(ts), Some(time)) = (input.event.ts.as_deref(), input.event.time.as_deref())
        && ts != time
    {
        errors.push("event.ts and event.time must match when both are provided".into());
    }

    if let Some(tenant) = input.event.meta.tenant.as_deref()
        && tenant.trim().is_empty()
    {
        errors.push("event.meta.tenant must not be empty when provided".into());
    }

    if input
        .event
        .meta
        .tags
        .iter()
        .any(|tag| tag.trim().is_empty())
    {
        errors.push("event.meta.tags entries must not be empty".into());
    }

    if let Some(options) = input.options.as_ref()
        && let Some(key) = options.dedupe_key.as_deref()
        && key.trim().is_empty()
    {
        errors.push("options.dedupe_key must not be empty when provided".into());
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(ValidationIssue::new(errors))
    }
}

fn is_rfc3339(value: &str) -> bool {
    DateTime::parse_from_rfc3339(value).is_ok()
}
