use greentic_types::TenantCtx;

use crate::error::{EventBusError, Result};
use crate::pattern::matches_pattern;

/// Rule controlling publish/subscribe permissions for a tenant on topic patterns.
#[derive(Clone, Debug)]
pub struct TopicAclRule {
    pub tenant_pattern: String,
    pub topic_pattern: String,
    pub allow_publish: bool,
    pub allow_subscribe: bool,
}

impl TopicAclRule {
    pub fn matches(&self, tenant: &TenantCtx, topic: &str) -> bool {
        matches_pattern(&self.tenant_pattern, tenant.tenant.as_str())
            && matches_pattern(&self.topic_pattern, topic)
    }
}

/// ACL collection evaluated before publish/subscribe.
#[derive(Clone, Debug, Default)]
pub struct TopicAcl {
    pub rules: Vec<TopicAclRule>,
    pub default_allow: bool,
}

impl TopicAcl {
    pub fn allow_publish(&self, tenant: &TenantCtx, topic: &str) -> Result<()> {
        if self
            .rules
            .iter()
            .find(|rule| rule.matches(tenant, topic))
            .map(|rule| rule.allow_publish)
            .unwrap_or(self.default_allow)
        {
            Ok(())
        } else {
            Err(EventBusError::AclDenied {
                tenant: tenant.tenant.to_string(),
                topic: topic.to_string(),
                operation: "publish",
            })
        }
    }

    pub fn allow_subscribe(&self, tenant: &TenantCtx, topic: &str) -> Result<()> {
        if self
            .rules
            .iter()
            .find(|rule| rule.matches(tenant, topic))
            .map(|rule| rule.allow_subscribe)
            .unwrap_or(self.default_allow)
        {
            Ok(())
        } else {
            Err(EventBusError::AclDenied {
                tenant: tenant.tenant.to_string(),
                topic: topic.to_string(),
                operation: "subscribe",
            })
        }
    }
}
