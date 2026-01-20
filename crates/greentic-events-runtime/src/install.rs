use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use futures::future::BoxFuture;
use greentic_events::backoff::RetryPolicy;
use greentic_events::provider::EventProvider;
use greentic_pack::events::{EventProviderKind, EventProviderSpec};
use greentic_types::{EventEnvelope, ProviderInstallId, ProviderInstallRecord};
use serde_json::{Map as JsonMap, Value as JsonValue};
use tracing::warn;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ProviderInstallKey {
    pub provider_id: String,
    pub install_id: ProviderInstallId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiagnosticCode {
    MissingConfig,
    MissingSecret,
    MalformedInstall,
    ProviderInitFailed,
    ProviderNotFound,
    SubscriptionSyncFailed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeDiagnostic {
    pub provider_id: String,
    pub install_id: Option<String>,
    pub code: DiagnosticCode,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct ResolvedInstall {
    pub record: ProviderInstallRecord,
    pub config: BTreeMap<String, JsonValue>,
    pub secrets: BTreeMap<String, JsonValue>,
}

#[async_trait]
pub trait ProviderInstallStore: Send + Sync {
    async fn list_installs(&self, provider_id: &str) -> Result<Vec<ProviderInstallRecord>>;
    async fn update_install(&self, record: ProviderInstallRecord) -> Result<()>;
}

#[async_trait]
pub trait InstallValueStore: Send + Sync {
    async fn read(&self, reference: &str) -> Result<JsonValue>;
}

#[async_trait]
pub trait InstallProviderFactory: Send + Sync {
    async fn build_provider(
        &self,
        spec: &EventProviderSpec,
        install: &ResolvedInstall,
    ) -> Result<Arc<dyn EventProvider>>;
}

#[async_trait]
pub trait SourceInitializer: Send + Sync {
    async fn init_source(&self, spec: &EventProviderSpec, install: &ResolvedInstall) -> Result<()>;
}

#[async_trait]
pub trait SubscriptionDriver: Send + Sync {
    async fn sync_subscriptions(
        &self,
        spec: &EventProviderSpec,
        install: &ResolvedInstall,
    ) -> Result<JsonValue>;
}

pub trait Clock: Send + Sync {
    fn now_rfc3339(&self) -> String;
}

pub trait Sleeper: Send + Sync {
    fn sleep(&self, duration: Duration) -> BoxFuture<'static, ()>;
}

pub struct InstallResolver<C, S> {
    config_store: Arc<C>,
    secret_store: Arc<S>,
}

impl<C, S> InstallResolver<C, S> {
    pub fn new(config_store: Arc<C>, secret_store: Arc<S>) -> Self {
        Self {
            config_store,
            secret_store,
        }
    }
}

impl<C, S> InstallResolver<C, S>
where
    C: InstallValueStore,
    S: InstallValueStore,
{
    pub async fn resolve(&self, record: &ProviderInstallRecord) -> Result<ResolvedInstall> {
        let mut config = BTreeMap::new();
        for (key, reference) in &record.config_refs {
            let value = self
                .config_store
                .read(reference)
                .await
                .with_context(|| format!("missing config ref {}", reference))?;
            config.insert(key.clone(), value);
        }

        let mut secrets = BTreeMap::new();
        for (key, reference) in &record.secret_refs {
            let value = self
                .secret_store
                .read(reference)
                .await
                .with_context(|| format!("missing secret ref {}", reference))?;
            secrets.insert(key.clone(), value);
        }

        Ok(ResolvedInstall {
            record: record.clone(),
            config,
            secrets,
        })
    }
}

#[derive(Default)]
pub struct InMemoryInstallStore {
    installs: Arc<Mutex<HashMap<ProviderInstallKey, ProviderInstallRecord>>>,
}

impl InMemoryInstallStore {
    pub fn insert(&self, record: ProviderInstallRecord) {
        let key = ProviderInstallKey {
            provider_id: record.provider_id.clone(),
            install_id: record.install_id.clone(),
        };
        self.installs.lock().unwrap().insert(key, record);
    }

    pub fn get(&self, key: &ProviderInstallKey) -> Option<ProviderInstallRecord> {
        self.installs.lock().unwrap().get(key).cloned()
    }
}

#[async_trait]
impl ProviderInstallStore for InMemoryInstallStore {
    async fn list_installs(&self, provider_id: &str) -> Result<Vec<ProviderInstallRecord>> {
        let installs = self
            .installs
            .lock()
            .unwrap()
            .values()
            .filter(|record| record.provider_id == provider_id)
            .cloned()
            .collect();
        Ok(installs)
    }

    async fn update_install(&self, record: ProviderInstallRecord) -> Result<()> {
        let key = ProviderInstallKey {
            provider_id: record.provider_id.clone(),
            install_id: record.install_id.clone(),
        };
        self.installs.lock().unwrap().insert(key, record);
        Ok(())
    }
}

#[derive(Default)]
pub struct InMemoryValueStore {
    values: Arc<Mutex<HashMap<String, JsonValue>>>,
}

impl InMemoryValueStore {
    pub fn insert(&self, reference: impl Into<String>, value: JsonValue) {
        self.values.lock().unwrap().insert(reference.into(), value);
    }
}

#[async_trait]
impl InstallValueStore for InMemoryValueStore {
    async fn read(&self, reference: &str) -> Result<JsonValue> {
        self.values
            .lock()
            .unwrap()
            .get(reference)
            .cloned()
            .ok_or_else(|| anyhow!("missing value for {}", reference))
    }
}

pub struct SinkRouter {
    providers: HashMap<ProviderInstallKey, Arc<dyn EventProvider>>,
}

impl SinkRouter {
    pub async fn emit(
        &self,
        provider_id: &str,
        install_id: &ProviderInstallId,
        event: EventEnvelope,
    ) -> Result<()> {
        let key = ProviderInstallKey {
            provider_id: provider_id.to_string(),
            install_id: install_id.clone(),
        };
        let provider = self
            .providers
            .get(&key)
            .ok_or_else(|| anyhow!("provider install not found: {}:{}", provider_id, install_id))?;
        provider.publish(event).await?;
        Ok(())
    }
}

pub struct InstallRuntime<R, C, S, F> {
    install_store: Arc<R>,
    resolver: InstallResolver<C, S>,
    factory: Arc<F>,
}

impl<R, C, S, F> InstallRuntime<R, C, S, F>
where
    R: ProviderInstallStore,
    C: InstallValueStore,
    S: InstallValueStore,
    F: InstallProviderFactory,
{
    pub fn new(install_store: Arc<R>, resolver: InstallResolver<C, S>, factory: Arc<F>) -> Self {
        Self {
            install_store,
            resolver,
            factory,
        }
    }

    pub async fn build_sink_router(
        &self,
        providers: &[EventProviderSpec],
    ) -> (SinkRouter, Vec<RuntimeDiagnostic>) {
        let mut diagnostics = Vec::new();
        let mut resolved = HashMap::new();

        for provider in providers
            .iter()
            .filter(|spec| matches!(spec.kind, EventProviderKind::Sink))
        {
            let installs = match self.install_store.list_installs(&provider.name).await {
                Ok(installs) => installs,
                Err(err) => {
                    diagnostics.push(RuntimeDiagnostic {
                        provider_id: provider.name.clone(),
                        install_id: None,
                        code: DiagnosticCode::MalformedInstall,
                        message: err.to_string(),
                    });
                    continue;
                }
            };

            for record in installs {
                let key = ProviderInstallKey {
                    provider_id: record.provider_id.clone(),
                    install_id: record.install_id.clone(),
                };
                let install = match self.resolver.resolve(&record).await {
                    Ok(install) => install,
                    Err(err) => {
                        diagnostics.push(RuntimeDiagnostic {
                            provider_id: record.provider_id.clone(),
                            install_id: Some(record.install_id.to_string()),
                            code: diagnostic_code_for_resolve_error(&err),
                            message: err.to_string(),
                        });
                        continue;
                    }
                };
                match self.factory.build_provider(provider, &install).await {
                    Ok(instance) => {
                        resolved.insert(key, instance);
                    }
                    Err(err) => {
                        diagnostics.push(RuntimeDiagnostic {
                            provider_id: record.provider_id.clone(),
                            install_id: Some(record.install_id.to_string()),
                            code: DiagnosticCode::ProviderInitFailed,
                            message: err.to_string(),
                        });
                    }
                }
            }
        }

        (
            SinkRouter {
                providers: resolved,
            },
            diagnostics,
        )
    }
}

pub struct SourceRuntime<R, C, S, H> {
    install_store: Arc<R>,
    resolver: InstallResolver<C, S>,
    initializer: Arc<H>,
}

impl<R, C, S, H> SourceRuntime<R, C, S, H>
where
    R: ProviderInstallStore,
    C: InstallValueStore,
    S: InstallValueStore,
    H: SourceInitializer,
{
    pub fn new(
        install_store: Arc<R>,
        resolver: InstallResolver<C, S>,
        initializer: Arc<H>,
    ) -> Self {
        Self {
            install_store,
            resolver,
            initializer,
        }
    }

    pub async fn start_sources(&self, providers: &[EventProviderSpec]) -> Vec<RuntimeDiagnostic> {
        let mut diagnostics = Vec::new();
        for provider in providers
            .iter()
            .filter(|spec| matches!(spec.kind, EventProviderKind::Source))
        {
            let installs = match self.install_store.list_installs(&provider.name).await {
                Ok(installs) => installs,
                Err(err) => {
                    diagnostics.push(RuntimeDiagnostic {
                        provider_id: provider.name.clone(),
                        install_id: None,
                        code: DiagnosticCode::MalformedInstall,
                        message: err.to_string(),
                    });
                    continue;
                }
            };

            for record in installs {
                let install = match self.resolver.resolve(&record).await {
                    Ok(install) => install,
                    Err(err) => {
                        diagnostics.push(RuntimeDiagnostic {
                            provider_id: record.provider_id.clone(),
                            install_id: Some(record.install_id.to_string()),
                            code: diagnostic_code_for_resolve_error(&err),
                            message: err.to_string(),
                        });
                        continue;
                    }
                };

                if let Err(err) = self.initializer.init_source(provider, &install).await {
                    warn!(
                        provider_id = %record.provider_id,
                        install_id = %record.install_id,
                        "source init failed: {}",
                        err
                    );
                    diagnostics.push(RuntimeDiagnostic {
                        provider_id: record.provider_id.clone(),
                        install_id: Some(record.install_id.to_string()),
                        code: DiagnosticCode::ProviderInitFailed,
                        message: err.to_string(),
                    });
                }
            }
        }
        diagnostics
    }
}

pub struct SubscriptionsWorker<R, C, S, D, Clk, Slp> {
    install_store: Arc<R>,
    resolver: InstallResolver<C, S>,
    driver: Arc<D>,
    clock: Arc<Clk>,
    sleeper: Arc<Slp>,
    retry: RetryPolicy,
    max_failures: u32,
}

impl<R, C, S, D, Clk, Slp> SubscriptionsWorker<R, C, S, D, Clk, Slp>
where
    R: ProviderInstallStore,
    C: InstallValueStore,
    S: InstallValueStore,
    D: SubscriptionDriver,
    Clk: Clock,
    Slp: Sleeper,
{
    pub fn new(
        install_store: Arc<R>,
        resolver: InstallResolver<C, S>,
        driver: Arc<D>,
        clock: Arc<Clk>,
        sleeper: Arc<Slp>,
    ) -> Self {
        Self {
            install_store,
            resolver,
            driver,
            clock,
            sleeper,
            retry: RetryPolicy::default(),
            max_failures: 3,
        }
    }

    pub fn with_retry_policy(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    pub fn with_max_failures(mut self, max: u32) -> Self {
        self.max_failures = max;
        self
    }

    pub async fn sync_all(
        &self,
        providers: &[EventProviderSpec],
        subscription_filter: impl Fn(&EventProviderSpec) -> bool,
    ) -> Vec<RuntimeDiagnostic> {
        let mut diagnostics = Vec::new();
        for provider in providers.iter().filter(|spec| subscription_filter(spec)) {
            let installs = match self.install_store.list_installs(&provider.name).await {
                Ok(installs) => installs,
                Err(err) => {
                    diagnostics.push(RuntimeDiagnostic {
                        provider_id: provider.name.clone(),
                        install_id: None,
                        code: DiagnosticCode::MalformedInstall,
                        message: err.to_string(),
                    });
                    continue;
                }
            };

            for record in installs {
                let result = self.sync_install(provider, record.clone()).await;
                if let Err(err) = result {
                    diagnostics.push(RuntimeDiagnostic {
                        provider_id: record.provider_id.clone(),
                        install_id: Some(record.install_id.to_string()),
                        code: DiagnosticCode::SubscriptionSyncFailed,
                        message: err.to_string(),
                    });
                }
            }
        }

        diagnostics
    }

    async fn sync_install(
        &self,
        spec: &EventProviderSpec,
        record: ProviderInstallRecord,
    ) -> Result<()> {
        let install = self.resolver.resolve(&record).await?;
        let mut last_err = None;
        for attempt in 0..=self.retry.max_retries {
            match self.driver.sync_subscriptions(spec, &install).await {
                Ok(state) => {
                    let mut updated = install.record.clone();
                    updated.subscriptions_state = state;
                    updated.metadata = update_subscription_metadata(
                        updated.metadata,
                        self.clock.now_rfc3339(),
                        0,
                        false,
                    );
                    self.install_store.update_install(updated).await?;
                    return Ok(());
                }
                Err(err) => {
                    last_err = Some(err);
                    if attempt == self.retry.max_retries {
                        break;
                    }
                    let delay = self.retry.delay_for_attempt(attempt + 1);
                    self.sleeper.sleep(delay).await;
                }
            }
        }

        let failures = increment_failure_metadata(&self.install_store, &record, self.max_failures)
            .await
            .unwrap_or(0);
        if failures >= self.max_failures {
            warn!(
                provider_id = %record.provider_id,
                install_id = %record.install_id,
                "subscription sync degraded after {} failures",
                failures
            );
        }
        Err(last_err
            .map(|err| anyhow!(err.to_string()))
            .unwrap_or_else(|| anyhow!("subscription sync failed")))
    }
}

fn diagnostic_code_for_resolve_error(err: &anyhow::Error) -> DiagnosticCode {
    let msg = err.to_string();
    if msg.contains("secret") {
        DiagnosticCode::MissingSecret
    } else if msg.contains("config") {
        DiagnosticCode::MissingConfig
    } else {
        DiagnosticCode::MalformedInstall
    }
}

fn update_subscription_metadata(
    metadata: JsonValue,
    last_sync: String,
    failures: u32,
    degraded: bool,
) -> JsonValue {
    let mut root = match metadata {
        JsonValue::Object(map) => map,
        _ => JsonMap::new(),
    };
    let mut subs = JsonMap::new();
    subs.insert("last_sync".into(), JsonValue::String(last_sync));
    subs.insert(
        "failures".into(),
        JsonValue::Number(serde_json::Number::from(failures)),
    );
    subs.insert("degraded".into(), JsonValue::Bool(degraded));
    root.insert("subscriptions".into(), JsonValue::Object(subs));
    JsonValue::Object(root)
}

async fn increment_failure_metadata<R: ProviderInstallStore>(
    store: &Arc<R>,
    record: &ProviderInstallRecord,
    max_failures: u32,
) -> Result<u32> {
    let mut updated = record.clone();
    let (mut failures, degraded) = failure_state(&updated.metadata);
    failures = failures.saturating_add(1);
    let degraded = degraded || failures >= max_failures;
    updated.metadata = update_subscription_metadata(
        updated.metadata.clone(),
        "unknown".into(),
        failures,
        degraded,
    );
    store.update_install(updated).await?;
    Ok(failures)
}

fn failure_state(metadata: &JsonValue) -> (u32, bool) {
    let JsonValue::Object(root) = metadata else {
        return (0, false);
    };
    let JsonValue::Object(subs) = root.get("subscriptions").unwrap_or(&JsonValue::Null) else {
        return (0, false);
    };
    let failures = subs
        .get("failures")
        .and_then(JsonValue::as_u64)
        .unwrap_or(0) as u32;
    let degraded = subs
        .get("degraded")
        .and_then(JsonValue::as_bool)
        .unwrap_or(false);
    (failures, degraded)
}
