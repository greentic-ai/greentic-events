use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use greentic_events::subscription::{SubscriptionHandle, SubscriptionOptions};
use greentic_events_runtime::install::{
    Clock, DiagnosticCode, InMemoryInstallStore, InMemoryValueStore, InstallProviderFactory,
    InstallResolver, InstallRuntime, ProviderInstallKey, ResolvedInstall, SubscriptionDriver,
    SubscriptionsWorker,
};
use greentic_pack::events::{EventProviderCapabilities, EventProviderKind, EventProviderSpec};
use greentic_types::{
    EventEnvelope, EventId, PackId, ProviderInstallId, ProviderInstallRecord, TenantCtx,
};
use serde_json::json;
use time::OffsetDateTime;

struct RecordingProvider {
    descriptor: greentic_types::EventProviderDescriptor,
    events: Arc<Mutex<Vec<EventEnvelope>>>,
}

#[async_trait]
impl greentic_events::provider::EventProvider for RecordingProvider {
    fn descriptor(&self) -> &greentic_types::EventProviderDescriptor {
        &self.descriptor
    }

    fn topic_patterns(&self) -> &[String] {
        &[]
    }

    async fn publish(&self, event: EventEnvelope) -> Result<()> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }

    async fn subscribe(
        &self,
        _topic: &str,
        _tenant: TenantCtx,
        _options: SubscriptionOptions,
    ) -> Result<SubscriptionHandle> {
        Err(anyhow::anyhow!("not supported"))
    }
}

struct RecordingFactory {
    providers: Arc<Mutex<BTreeMap<String, Arc<RecordingProvider>>>>,
}

#[async_trait]
impl InstallProviderFactory for RecordingFactory {
    async fn build_provider(
        &self,
        _spec: &EventProviderSpec,
        install: &ResolvedInstall,
    ) -> Result<Arc<dyn greentic_events::provider::EventProvider>> {
        let provider = Arc::new(RecordingProvider {
            descriptor: greentic_types::EventProviderDescriptor {
                name: "dummy".into(),
                kind: greentic_types::EventProviderKind::Sink,
                transport: greentic_types::TransportKind::Other("test".into()),
                reliability: greentic_types::ReliabilityKind::AtMostOnce,
                ordering: greentic_types::OrderingKind::None,
                notes: None,
                tags: Vec::new(),
            },
            events: Arc::new(Mutex::new(Vec::new())),
        });
        self.providers
            .lock()
            .unwrap()
            .insert(install.record.install_id.to_string(), Arc::clone(&provider));
        Ok(provider)
    }
}

fn sample_spec() -> EventProviderSpec {
    EventProviderSpec {
        name: "events.github".to_string(),
        kind: EventProviderKind::Sink,
        component: "github@1.0.0".to_string(),
        default_flow: None,
        custom_flow: None,
        capabilities: EventProviderCapabilities {
            topics: vec!["greentic.test".to_string()],
            ..Default::default()
        },
    }
}

fn install_record(install_id: &str) -> ProviderInstallRecord {
    ProviderInstallRecord {
        tenant: TenantCtx::new("dev".try_into().unwrap(), "tenant".try_into().unwrap()),
        provider_id: "events.github".to_string(),
        install_id: ProviderInstallId::try_from(install_id).unwrap(),
        pack_id: PackId::try_from("events-github").unwrap(),
        pack_version: "1.0.0".parse().unwrap(),
        created_at: OffsetDateTime::UNIX_EPOCH,
        updated_at: OffsetDateTime::UNIX_EPOCH,
        config_refs: BTreeMap::new(),
        secret_refs: BTreeMap::new(),
        webhook_state: serde_json::Value::Null,
        subscriptions_state: serde_json::Value::Null,
        metadata: serde_json::Value::Null,
    }
}

#[tokio::test]
async fn multiple_installs_route_correctly() -> Result<()> {
    let install_store = Arc::new(InMemoryInstallStore::default());
    install_store.insert(install_record("install-a"));
    install_store.insert(install_record("install-b"));

    let config_store = Arc::new(InMemoryValueStore::default());
    let secret_store = Arc::new(InMemoryValueStore::default());
    let resolver = InstallResolver::new(config_store, secret_store);

    let providers = Arc::new(Mutex::new(BTreeMap::new()));
    let factory = Arc::new(RecordingFactory {
        providers: Arc::clone(&providers),
    });
    let runtime = InstallRuntime::new(install_store, resolver, factory);

    let (router, diagnostics) = runtime.build_sink_router(&[sample_spec()]).await;
    assert!(diagnostics.is_empty());

    let event = EventEnvelope {
        id: EventId::new("evt-1").unwrap(),
        topic: "greentic.test".to_string(),
        r#type: "greentic.events.test".to_string(),
        source: "test".to_string(),
        tenant: TenantCtx::new("dev".try_into().unwrap(), "tenant".try_into().unwrap()),
        subject: None,
        time: chrono::Utc::now(),
        correlation_id: None,
        payload: json!({"ok": true}),
        metadata: Default::default(),
    };

    router
        .emit(
            "events.github",
            &ProviderInstallId::try_from("install-a").unwrap(),
            event.clone(),
        )
        .await?;
    router
        .emit(
            "events.github",
            &ProviderInstallId::try_from("install-b").unwrap(),
            event.clone(),
        )
        .await?;

    let providers = providers.lock().unwrap();
    assert_eq!(providers.len(), 2);
    assert_eq!(providers["install-a"].events.lock().unwrap().len(), 1);
    assert_eq!(providers["install-b"].events.lock().unwrap().len(), 1);
    Ok(())
}

#[tokio::test]
async fn missing_secret_produces_diagnostic() -> Result<()> {
    let install_store = Arc::new(InMemoryInstallStore::default());
    let mut record = install_record("install-missing");
    record
        .secret_refs
        .insert("token".into(), "secret://missing".into());
    install_store.insert(record);

    let config_store = Arc::new(InMemoryValueStore::default());
    let secret_store = Arc::new(InMemoryValueStore::default());
    let resolver = InstallResolver::new(config_store, secret_store);
    let factory = Arc::new(RecordingFactory {
        providers: Arc::new(Mutex::new(BTreeMap::new())),
    });
    let runtime = InstallRuntime::new(install_store, resolver, factory);

    let (_router, diagnostics) = runtime.build_sink_router(&[sample_spec()]).await;
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, DiagnosticCode::MissingSecret);
    Ok(())
}

struct FixedClock;

impl Clock for FixedClock {
    fn now_rfc3339(&self) -> String {
        "2025-01-01T00:00:00Z".to_string()
    }
}

struct NoopSleeper;

impl greentic_events_runtime::install::Sleeper for NoopSleeper {
    fn sleep(&self, _duration: std::time::Duration) -> futures::future::BoxFuture<'static, ()> {
        Box::pin(async move {})
    }
}

struct StubSubscriptionDriver;

#[async_trait]
impl SubscriptionDriver for StubSubscriptionDriver {
    async fn sync_subscriptions(
        &self,
        _spec: &EventProviderSpec,
        _install: &ResolvedInstall,
    ) -> Result<serde_json::Value> {
        Ok(json!({
            "subscriptions": [
                {"id": "sub-1", "expires_at": "2025-01-02T00:00:00Z"}
            ]
        }))
    }
}

#[tokio::test]
async fn subscriptions_worker_updates_install_record() -> Result<()> {
    let install_store = Arc::new(InMemoryInstallStore::default());
    install_store.insert(install_record("install-sub"));

    let config_store = Arc::new(InMemoryValueStore::default());
    let secret_store = Arc::new(InMemoryValueStore::default());
    let resolver = InstallResolver::new(config_store, secret_store);
    let worker = SubscriptionsWorker::new(
        Arc::clone(&install_store),
        resolver,
        Arc::new(StubSubscriptionDriver),
        Arc::new(FixedClock),
        Arc::new(NoopSleeper),
    );

    let diagnostics = worker.sync_all(&[sample_spec()], |_| true).await;
    assert!(diagnostics.is_empty());

    let key = ProviderInstallKey {
        provider_id: "events.github".to_string(),
        install_id: ProviderInstallId::try_from("install-sub").unwrap(),
    };
    let updated = install_store.get(&key).expect("updated record");
    assert!(updated.subscriptions_state.is_object());
    let metadata = updated.metadata.as_object().expect("metadata object");
    let subscriptions = metadata
        .get("subscriptions")
        .and_then(|value| value.as_object())
        .expect("subscriptions metadata");
    assert_eq!(
        subscriptions.get("last_sync").unwrap().as_str().unwrap(),
        "2025-01-01T00:00:00Z"
    );
    Ok(())
}
