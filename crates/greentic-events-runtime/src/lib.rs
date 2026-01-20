use greentic_config_types::{
    BackoffConfig, GreenticConfig, NetworkConfig, ReconnectConfig, ServiceEndpointConfig,
    TelemetryConfig,
};
use greentic_events::backoff::{BackoffStrategy, RetryPolicy};
use std::time::Duration;

pub mod install;

/// Runtime-facing view of event settings derived from `GreenticConfig`.
/// Keeps the core `greentic-events` crate free from config-loader concerns.
#[derive(Clone, Debug, PartialEq)]
pub struct EventsRuntimeConfig {
    pub endpoint: Option<ServiceEndpointConfig>,
    pub reconnect: Option<ReconnectConfig>,
    pub backoff: Option<BackoffConfig>,
    pub telemetry: TelemetryConfig,
    pub network: NetworkConfig,
}

impl EventsRuntimeConfig {
    /// Build runtime config from a resolved `GreenticConfig`.
    pub fn from_greentic(config: &GreenticConfig) -> Self {
        Self {
            endpoint: config
                .services
                .as_ref()
                .and_then(|services| services.events.clone()),
            reconnect: config
                .events
                .as_ref()
                .and_then(|events| events.reconnect.clone()),
            backoff: config
                .events
                .as_ref()
                .and_then(|events| events.backoff.clone()),
            telemetry: config.telemetry.clone(),
            network: config.network.clone(),
        }
    }

    /// Convenience accessor to the configured events endpoint URL (if any).
    pub fn endpoint_url(&self) -> Option<&str> {
        self.endpoint.as_ref().map(|ep| ep.url.as_str())
    }

    /// Map reconnect/backoff settings to the crate's retry policy.
    pub fn to_retry_policy(&self) -> RetryPolicy {
        retry_policy_from(self)
    }
}

/// Produce a retry policy from the runtime config using reasonable defaults where fields are absent.
pub fn retry_policy_from(runtime: &EventsRuntimeConfig) -> RetryPolicy {
    let mut policy = RetryPolicy::default();

    if let Some(reconnect) = &runtime.reconnect
        && let Some(max) = reconnect.max_retries
    {
        policy.max_retries = max;
    }

    if let Some(backoff) = &runtime.backoff {
        if let Some(initial) = backoff.initial_ms {
            policy.base_delay = Duration::from_millis(initial);
        }
        if let Some(max) = backoff.max_ms {
            policy.max_delay = Duration::from_millis(max);
        }
        let jitter = backoff.jitter.unwrap_or(true);
        if let Some(multiplier) = backoff.multiplier {
            policy.strategy = if multiplier <= 1.0 {
                BackoffStrategy::Fixed
            } else if jitter {
                BackoffStrategy::ExponentialWithJitter
            } else {
                BackoffStrategy::Exponential
            };
        } else if backoff.jitter.is_some() {
            policy.strategy = if jitter {
                BackoffStrategy::ExponentialWithJitter
            } else {
                BackoffStrategy::Exponential
            };
        }
    }

    policy
}

/// Load, validate, and map a `GreenticConfig` into runtime settings (requires the `resolver` feature).
#[cfg(feature = "resolver")]
pub fn resolve_runtime_config(
    project_root: Option<std::path::PathBuf>,
    allow_dev: bool,
) -> anyhow::Result<(EventsRuntimeConfig, greentic_config::ResolvedConfig)> {
    let resolver = greentic_config::ConfigResolver::new()
        .with_project_root_opt(project_root)
        .allow_dev(allow_dev);
    let resolved = resolver.load()?;
    let runtime = EventsRuntimeConfig::from_greentic(&resolved.config);
    Ok((runtime, resolved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use greentic_config_types::{
        ConfigVersion, EnvironmentConfig, EventsConfig, NetworkConfig, PathsConfig,
        SecretsBackendRefConfig, ServiceEndpointConfig, ServicesConfig, TelemetryConfig,
    };
    use greentic_types::EnvId;
    use std::path::PathBuf;
    use url::Url;

    fn sample_config() -> GreenticConfig {
        GreenticConfig {
            schema_version: ConfigVersion::v1(),
            environment: EnvironmentConfig {
                env_id: EnvId::try_from("dev").unwrap(),
                deployment: None,
                connection: None,
                region: Some("us-east-1".into()),
            },
            paths: PathsConfig {
                greentic_root: PathBuf::from("/workspace"),
                state_dir: PathBuf::from("/workspace/.greentic"),
                cache_dir: PathBuf::from("/workspace/.greentic/cache"),
                logs_dir: PathBuf::from("/workspace/.greentic/logs"),
            },
            packs: None,
            services: Some(ServicesConfig {
                events: Some(ServiceEndpointConfig {
                    url: Url::parse("https://events.greentic.test").unwrap(),
                    headers: None,
                }),
                ..Default::default()
            }),
            events: Some(EventsConfig {
                reconnect: Some(ReconnectConfig {
                    enabled: Some(true),
                    max_retries: Some(7),
                }),
                backoff: Some(BackoffConfig {
                    initial_ms: Some(250),
                    max_ms: Some(5_000),
                    multiplier: Some(2.0),
                    jitter: Some(false),
                }),
            }),
            runtime: greentic_config_types::RuntimeConfig::default(),
            telemetry: TelemetryConfig::default(),
            network: NetworkConfig::default(),
            deployer: None,
            secrets: SecretsBackendRefConfig::default(),
            dev: None,
        }
    }

    #[test]
    fn maps_services_and_events() {
        let config = sample_config();
        let runtime = EventsRuntimeConfig::from_greentic(&config);

        assert_eq!(
            runtime.endpoint_url(),
            Some("https://events.greentic.test/")
        );
        assert!(runtime.reconnect.is_some());
        assert!(runtime.backoff.is_some());
        assert_eq!(runtime.telemetry, config.telemetry);
        assert_eq!(runtime.network, config.network);
    }

    #[test]
    fn retry_policy_uses_backoff_and_reconnect() {
        let config = sample_config();
        let runtime = EventsRuntimeConfig::from_greentic(&config);
        let policy = runtime.to_retry_policy();

        assert_eq!(policy.max_retries, 7);
        assert_eq!(policy.base_delay, Duration::from_millis(250));
        assert_eq!(policy.max_delay, Duration::from_millis(5_000));
        assert_eq!(policy.strategy, BackoffStrategy::Exponential);
    }

    #[cfg(feature = "resolver")]
    #[test]
    fn resolve_runtime_config_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let (runtime, resolved) =
            resolve_runtime_config(Some(tmp.path().to_path_buf()), true).expect("resolve config");
        // Defaults produce absolute paths via resolver; endpoint is optional by default.
        assert!(resolved.config.paths.state_dir.is_absolute());
        assert!(runtime.telemetry.enabled);
    }
}
