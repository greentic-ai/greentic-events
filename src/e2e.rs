use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use futures::FutureExt;
use greentic_flow::template::TemplateRenderer;
use greentic_flow::{compile_flow, loader as flow_loader};
use greentic_pack::events::{EventProviderKind, EventProviderSpec};
use greentic_pack::reader::{PackLoad, SigningPolicy, open_pack};
use greentic_types::{
    ChannelMessageEnvelope, EventEnvelope, EventId, EventProviderDescriptor, TenantCtx,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_yaml_bw;
use tokio::sync::broadcast;
use tokio::time::timeout;

use crate::bridge::{BridgeRegistry, BridgeService, EventToMessageBridge, MessageToEventBridge};
use crate::provider::{
    EventProvider, EventProviderFactory, ProviderRegistration, descriptor_from_spec,
};
use crate::subscription::{AckMode, SubscriptionHandle, SubscriptionOptions, subscription_channel};
use std::panic::AssertUnwindSafe;
use std::process::Command;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
const FIXED_TIME: &str = "2025-01-01T00:00:00Z";
const LIVE_BRIDGE_CMD_ENV: &str = "GREENTIC_EVENTS_E2E_LIVE_BRIDGE_CMD";
const LIVE_BROKER_CMD_ENV: &str = "GREENTIC_EVENTS_E2E_LIVE_BROKER_CMD";

#[derive(Debug, Clone)]
pub struct E2eOptions {
    pub packs_dir: PathBuf,
    pub provider_filter: Option<String>,
    pub report_path: Option<PathBuf>,
    pub dry_run: bool,
    pub live: bool,
}

#[derive(Clone)]
pub struct E2eDependencies {
    requirements_runner: Arc<dyn RequirementsRunner>,
    provision_runner: Arc<dyn ProvisionRunner>,
    ingest_runner: Arc<dyn IngestRunner>,
    emit_runner: Arc<dyn EmitRunner>,
    subscription_runner: Arc<dyn SubscriptionRunner>,
}

impl E2eDependencies {
    pub fn new(
        requirements_runner: Arc<dyn RequirementsRunner>,
        provision_runner: Arc<dyn ProvisionRunner>,
        ingest_runner: Arc<dyn IngestRunner>,
        emit_runner: Arc<dyn EmitRunner>,
        subscription_runner: Arc<dyn SubscriptionRunner>,
    ) -> Self {
        Self {
            requirements_runner,
            provision_runner,
            ingest_runner,
            emit_runner,
            subscription_runner,
        }
    }
}

impl Default for E2eDependencies {
    fn default() -> Self {
        Self {
            requirements_runner: Arc::new(CompositeRequirementsRunner::default()),
            provision_runner: Arc::new(CompositeProvisionRunner::default()),
            ingest_runner: Arc::new(FixtureIngestRunner),
            emit_runner: Arc::new(FixtureEmitRunner),
            subscription_runner: Arc::new(FixtureSubscriptionRunner),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct E2eReport {
    pub summary: E2eSummary,
    pub packs: Vec<PackReport>,
}

#[derive(Debug, Serialize)]
pub struct E2eSummary {
    pub total: usize,
    pub pass: usize,
    pub fail: usize,
    pub skip: usize,
}

#[derive(Debug, Serialize)]
pub struct PackReport {
    pub pack: String,
    pub version: String,
    pub status: StageStatus,
    pub stages: StageReport,
    pub diagnostics: Vec<E2eDiagnostic>,
    pub timing_ms: TimingMs,
}

#[derive(Debug, Serialize)]
pub struct StageReport {
    pub requirements: StageStatus,
    pub setup: StageStatus,
    pub ingest: StageStatus,
    pub emit: StageStatus,
    pub subscriptions: StageStatus,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StageStatus {
    Pass,
    Fail,
    Skip,
}

#[derive(Debug, Serialize)]
pub struct E2eDiagnostic {
    pub stage: Option<String>,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct TimingMs {
    pub total: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RequirementsFixture {
    #[serde(default)]
    pub required_config_keys: Vec<String>,
    #[serde(default)]
    pub required_secret_keys: Vec<String>,
    #[serde(default)]
    pub subscriptions_required: bool,
    #[serde(default)]
    pub webhooks_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProvisionPlan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config_patch: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    secrets_patch: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    subscriptions: Vec<SubscriptionOp>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    webhooks: Vec<WebhookOp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionAction {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubscriptionOp {
    action: SubscriptionAction,
    topic: String,
    endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WebhookAction {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebhookOp {
    action: WebhookAction,
    name: String,
    url: String,
    events: Vec<String>,
}

#[async_trait::async_trait]
pub trait RequirementsRunner: Send + Sync {
    async fn run(&self, pack: &PackLoad, flow_id: Option<&str>) -> Result<RequirementsFixture>;
}

#[derive(Default)]
struct FixtureRequirementsRunner;

#[async_trait::async_trait]
impl RequirementsRunner for FixtureRequirementsRunner {
    async fn run(&self, pack: &PackLoad, _flow_id: Option<&str>) -> Result<RequirementsFixture> {
        load_fixture::<RequirementsFixture>(pack, "fixtures/requirements.expected.json")
            .context("requirements fixture missing or invalid")
    }
}

#[derive(Default)]
struct CompositeRequirementsRunner {
    fixture: FixtureRequirementsRunner,
}

#[async_trait::async_trait]
impl RequirementsRunner for CompositeRequirementsRunner {
    async fn run(&self, pack: &PackLoad, flow_id: Option<&str>) -> Result<RequirementsFixture> {
        if let Some(flow_id) = flow_id
            && let Some(output) = run_config_flow_from_pack(pack, flow_id, None)?
            && let Ok(requirements) = parse_requirements_from_output(&output)
        {
            return Ok(requirements);
        }
        self.fixture.run(pack, flow_id).await
    }
}

#[async_trait::async_trait]
pub trait ProvisionRunner: Send + Sync {
    async fn plan(&self, pack: &PackLoad, inputs: &ProvisionInputs) -> Result<ProvisionPlan>;
}

#[derive(Default)]
struct FixtureProvisionRunner;

#[async_trait::async_trait]
impl ProvisionRunner for FixtureProvisionRunner {
    async fn plan(&self, pack: &PackLoad, inputs: &ProvisionInputs) -> Result<ProvisionPlan> {
        let _setup_input: Value = load_fixture(pack, "fixtures/setup.input.json")
            .context("setup fixture missing or invalid")?;
        let subscriptions = if inputs.requirements.subscriptions_required {
            let expected: Value = load_fixture(pack, "fixtures/subscriptions.expected.json")
                .context("subscriptions fixture missing")?;
            let Value::Array(items) = expected else {
                anyhow::bail!("subscriptions expected fixture must be a JSON array");
            };
            items
                .into_iter()
                .enumerate()
                .map(|(idx, value)| {
                    let topic = value
                        .get("topic")
                        .and_then(Value::as_str)
                        .unwrap_or(&format!("fixture.topic.{idx}"))
                        .to_string();
                    let endpoint = value
                        .get("endpoint")
                        .and_then(Value::as_str)
                        .unwrap_or("fixture.endpoint")
                        .to_string();
                    SubscriptionOp {
                        action: SubscriptionAction::Create,
                        topic,
                        endpoint,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        Ok(ProvisionPlan {
            config_patch: None,
            secrets_patch: None,
            subscriptions,
            webhooks: Vec::new(),
        })
    }
}

#[derive(Default)]
struct CompositeProvisionRunner {
    fixture: FixtureProvisionRunner,
}

#[async_trait::async_trait]
impl ProvisionRunner for CompositeProvisionRunner {
    async fn plan(&self, pack: &PackLoad, inputs: &ProvisionInputs) -> Result<ProvisionPlan> {
        if let Some(flow_id) = setup_flow_id(pack)
            && let Some(output) = run_config_flow_from_pack(pack, &flow_id, Some(&inputs.answers))?
            && let Ok(plan) = parse_provision_plan_from_output(&output)
        {
            return Ok(plan);
        }
        self.fixture.plan(pack, inputs).await
    }
}

#[async_trait::async_trait]
pub trait IngestRunner: Send + Sync {
    async fn ingest(
        &self,
        pack: &PackLoad,
        providers: &[EventProviderSpec],
        live: bool,
    ) -> Result<Vec<EventEnvelope>>;
}

struct FixtureIngestRunner;

#[async_trait::async_trait]
impl IngestRunner for FixtureIngestRunner {
    async fn ingest(
        &self,
        pack: &PackLoad,
        providers: &[EventProviderSpec],
        live: bool,
    ) -> Result<Vec<EventEnvelope>> {
        if live {
            let cmd = env_command(LIVE_BRIDGE_CMD_ENV)?;
            return command_ingest(cmd, pack, providers).await;
        }
        let message: ChannelMessageEnvelope = load_fixture(pack, "fixtures/ingest.request.json")
            .context("ingest fixture missing or invalid")?;
        let provider_name = providers
            .iter()
            .find(|provider| {
                matches!(
                    provider.kind,
                    EventProviderKind::Source | EventProviderKind::Bridge
                )
            })
            .map(|provider| provider.name.clone())
            .unwrap_or_else(|| "events".to_string());
        let bridge = DryRunMessageToEventBridge { provider_name };
        let mut registry = BridgeRegistry::new();
        registry.register_message_to_event("ingest", Arc::new(bridge));
        let service = BridgeService::new(registry);
        service
            .handle_message("ingest", message)
            .await
            .context("ingest bridge failed")
    }
}

#[async_trait::async_trait]
pub trait EmitRunner: Send + Sync {
    async fn emit(
        &self,
        pack: &PackLoad,
        providers: &[EventProviderSpec],
        live: bool,
    ) -> Result<Vec<ChannelMessageEnvelope>>;
}

struct FixtureEmitRunner;

#[async_trait::async_trait]
impl EmitRunner for FixtureEmitRunner {
    async fn emit(
        &self,
        pack: &PackLoad,
        providers: &[EventProviderSpec],
        live: bool,
    ) -> Result<Vec<ChannelMessageEnvelope>> {
        if live {
            let cmd = env_command(LIVE_BRIDGE_CMD_ENV)?;
            return command_emit(cmd, pack, providers).await;
        }
        let event: EventEnvelope = load_fixture(pack, "fixtures/emit.request.json")
            .context("emit fixture missing or invalid")?;
        let provider_name = providers
            .iter()
            .find(|provider| {
                matches!(
                    provider.kind,
                    EventProviderKind::Sink | EventProviderKind::Bridge
                )
            })
            .map(|provider| provider.name.clone())
            .unwrap_or_else(|| "events".to_string());
        let bridge = DryRunEventToMessageBridge { provider_name };
        let mut registry = BridgeRegistry::new();
        registry.register_event_to_message("emit", Arc::new(bridge));
        let service = BridgeService::new(registry);
        service
            .handle_event("emit", event)
            .await
            .context("emit bridge failed")
    }
}

#[async_trait::async_trait]
pub trait SubscriptionRunner: Send + Sync {
    async fn sync(
        &self,
        pack: &PackLoad,
        providers: &[EventProviderSpec],
        live: bool,
    ) -> Result<()>;
}

struct FixtureSubscriptionRunner;

#[async_trait::async_trait]
impl SubscriptionRunner for FixtureSubscriptionRunner {
    async fn sync(
        &self,
        pack: &PackLoad,
        providers: &[EventProviderSpec],
        live: bool,
    ) -> Result<()> {
        if live {
            let cmd = env_command(LIVE_BROKER_CMD_ENV)?;
            return command_subscriptions(cmd, pack, providers).await;
        }
        let expected: Value = load_fixture(pack, "fixtures/subscriptions.expected.json")
            .context("subscriptions fixture missing or invalid")?;
        if !expected.is_array() {
            return Err(anyhow!(
                "subscriptions expected fixture must be a JSON array"
            ));
        }
        let broker_provider = providers
            .iter()
            .find(|provider| matches!(provider.kind, EventProviderKind::Broker))
            .ok_or_else(|| anyhow!("subscriptions requested but no broker providers declared"))?;

        let factory = DryRunProviderFactory::default();
        let registration = registration_from_spec(broker_provider, &factory).await?;
        let bus = crate::bus::EventBusBuilder::new()
            .register_provider(registration)
            .build();

        let topic = match broker_provider.capabilities.topics.first() {
            Some(pattern) => sample_topic(pattern),
            None => "greentic.events.test".to_string(),
        };

        let tenant = TenantCtx::new(
            "dev".try_into().map_err(|_| anyhow!("invalid env id"))?,
            "tenant"
                .try_into()
                .map_err(|_| anyhow!("invalid tenant id"))?,
        );
        let options = SubscriptionOptions {
            durable: false,
            deliver_existing: false,
            ack_mode: AckMode::Auto,
        };
        let mut subscription = bus.subscribe(&topic, tenant.clone(), options).await?;

        let time = DateTime::parse_from_rfc3339(FIXED_TIME)
            .map_err(|err| anyhow!(err))?
            .with_timezone(&Utc);
        let event = EventEnvelope {
            id: EventId::new("evt-subscription")?,
            topic: topic.clone(),
            r#type: "greentic.events.subscription".into(),
            source: "greentic-events-test".into(),
            tenant,
            subject: Some("subscription".into()),
            time,
            correlation_id: None,
            payload: Value::Object(Default::default()),
            metadata: Default::default(),
        };
        bus.publish(event).await?;

        let received = timeout(Duration::from_millis(500), subscription.next())
            .await
            .context("subscription receive timed out")?
            .context("subscription channel closed")?;
        if received.topic.trim().is_empty() {
            return Err(anyhow!("subscription received empty topic"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ProvisionInputs {
    pub provider_id: String,
    pub install_id: String,
    pub public_base_url: Option<String>,
    pub answers: Value,
    pub requirements: RequirementsFixture,
}

#[derive(Debug, Clone)]
struct SetupSummary {
    config_patches: usize,
    secrets_patches: usize,
    subscriptions: usize,
    webhooks: usize,
}

#[derive(Clone)]
struct DryRunProvider {
    descriptor: EventProviderDescriptor,
    topics: Vec<String>,
    sender: broadcast::Sender<EventEnvelope>,
    published: Arc<Mutex<Vec<EventEnvelope>>>,
}

impl DryRunProvider {
    fn new(spec: &EventProviderSpec) -> Self {
        let descriptor = descriptor_from_spec(spec);
        let topics = spec.capabilities.topics.clone();
        let (sender, _) = broadcast::channel(32);
        Self {
            descriptor,
            topics,
            sender,
            published: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait::async_trait]
impl EventProvider for DryRunProvider {
    fn descriptor(&self) -> &EventProviderDescriptor {
        &self.descriptor
    }

    fn topic_patterns(&self) -> &[String] {
        &self.topics
    }

    async fn publish(&self, event: EventEnvelope) -> Result<()> {
        self.published.lock().unwrap().push(event.clone());
        let _ = self.sender.send(event);
        Ok(())
    }

    async fn subscribe(
        &self,
        _topic: &str,
        _tenant: TenantCtx,
        options: SubscriptionOptions,
    ) -> Result<SubscriptionHandle> {
        let (tx, handle) = subscription_channel();
        if options.deliver_existing {
            let existing = self.published.lock().unwrap().clone();
            for ev in existing {
                if tx.send(ev).await.is_err() {
                    break;
                }
            }
        }
        let mut rx = self.sender.subscribe();
        tokio::spawn(async move {
            while let Ok(ev) = rx.recv().await {
                if tx.send(ev).await.is_err() {
                    break;
                }
            }
        });
        Ok(handle)
    }
}

#[derive(Default)]
struct DryRunProviderFactory {
    providers: Arc<Mutex<HashMap<String, Arc<DryRunProvider>>>>,
}

#[async_trait::async_trait]
impl EventProviderFactory for DryRunProviderFactory {
    async fn build_provider(&self, spec: &EventProviderSpec) -> Result<Arc<dyn EventProvider>> {
        let provider = Arc::new(DryRunProvider::new(spec));
        self.providers
            .lock()
            .unwrap()
            .insert(spec.name.clone(), Arc::clone(&provider));
        Ok(provider)
    }
}

struct DryRunMessageToEventBridge {
    provider_name: String,
}

#[async_trait::async_trait]
impl MessageToEventBridge for DryRunMessageToEventBridge {
    async fn handle_message(&self, message: ChannelMessageEnvelope) -> Result<Vec<EventEnvelope>> {
        let time = DateTime::parse_from_rfc3339(FIXED_TIME)
            .map_err(|err| anyhow!(err))?
            .with_timezone(&Utc);
        let ev = EventEnvelope {
            id: EventId::new(format!("evt-{}", message.id))?,
            topic: format!("{}.ingest", self.provider_name),
            r#type: "greentic.events.ingest".into(),
            source: "greentic-events-test".into(),
            tenant: message.tenant.clone(),
            subject: Some(message.session_id.clone()),
            time,
            correlation_id: message.correlation_id.clone(),
            payload: serde_json::to_value(&message)?,
            metadata: message.metadata.clone(),
        };
        Ok(vec![ev])
    }
}

struct DryRunEventToMessageBridge {
    provider_name: String,
}

#[async_trait::async_trait]
impl EventToMessageBridge for DryRunEventToMessageBridge {
    async fn handle_event(&self, event: EventEnvelope) -> Result<Vec<ChannelMessageEnvelope>> {
        let msg = ChannelMessageEnvelope {
            id: format!("msg-{}", event.id),
            tenant: event.tenant.clone(),
            channel: format!("{}.emit", self.provider_name),
            session_id: event.subject.clone().unwrap_or_else(|| "session".into()),
            reply_scope: None,
            user_id: None,
            correlation_id: event.correlation_id.clone(),
            text: Some(event.payload.to_string()),
            attachments: Vec::new(),
            metadata: event.metadata.clone(),
        };
        Ok(vec![msg])
    }
}

pub async fn run_e2e(options: E2eOptions) -> Result<E2eReport> {
    run_e2e_with(options, E2eDependencies::default()).await
}

pub async fn run_e2e_with(options: E2eOptions, deps: E2eDependencies) -> Result<E2eReport> {
    if options.live && options.dry_run {
        return Err(anyhow!("--live conflicts with --dry-run"));
    }
    apply_dry_run_guard(&options)?;
    let packs = discover_packs(&options.packs_dir)?;
    if packs.is_empty() {
        return Err(anyhow!("no packs found in {}", options.packs_dir.display()));
    }

    let mut reports = Vec::new();
    for pack in packs {
        let report = timeout(DEFAULT_TIMEOUT, run_pack(&pack, &options, &deps)).await;
        let report = match report {
            Ok(result) => result,
            Err(_) => Err(anyhow!("pack timed out after {:?}", DEFAULT_TIMEOUT)),
        };
        reports.push(report_to_pack_report(&pack, report));
    }

    let summary = summarize(&reports);
    let report = E2eReport {
        summary,
        packs: reports,
    };

    if let Some(path) = &options.report_path {
        let payload = serde_json::to_string_pretty(&report)?;
        std::fs::write(path, payload)
            .with_context(|| format!("write report to {}", path.display()))?;
    }

    Ok(report)
}

fn discover_packs(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut packs = Vec::new();
    for entry in
        std::fs::read_dir(dir).with_context(|| format!("read packs directory {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name,
            None => continue,
        };
        if name.starts_with("events-")
            && path.extension().and_then(|s| s.to_str()) == Some("gtpack")
        {
            packs.push(path);
        }
    }
    packs.sort();
    Ok(packs)
}

async fn run_pack(
    pack_path: &Path,
    options: &E2eOptions,
    deps: &E2eDependencies,
) -> Result<PackReport> {
    let started = Instant::now();
    let mut diagnostics = Vec::new();

    let pack = open_pack(pack_path, SigningPolicy::DevOk)
        .map_err(|err| anyhow!(err.message))
        .with_context(|| format!("open pack {}", pack_path.display()))?;
    let meta = &pack.manifest.meta;
    let pack_id = meta.pack_id.clone();
    let version = meta.version.to_string();

    let events = meta
        .events
        .as_ref()
        .ok_or_else(|| anyhow!("pack has no events section"))?;

    let mut providers = events.providers.clone();
    if let Some(filter) = &options.provider_filter {
        providers.retain(|provider| provider.name == *filter);
    }
    if providers.is_empty() {
        return Err(anyhow!("no matching event providers in pack"));
    }

    let has_source = providers
        .iter()
        .any(|p| matches!(p.kind, EventProviderKind::Source));
    let has_sink = providers
        .iter()
        .any(|p| matches!(p.kind, EventProviderKind::Sink));
    let has_bridge = providers
        .iter()
        .any(|p| matches!(p.kind, EventProviderKind::Bridge));
    let has_broker = providers
        .iter()
        .any(|p| matches!(p.kind, EventProviderKind::Broker));

    let mut stages = StageReport {
        requirements: StageStatus::Skip,
        setup: StageStatus::Skip,
        ingest: StageStatus::Skip,
        emit: StageStatus::Skip,
        subscriptions: StageStatus::Skip,
    };

    let requirements_flow = requirements_flow_id(&pack);
    let requirements =
        AssertUnwindSafe(run_requirements(&pack, requirements_flow.as_deref(), deps))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Err(anyhow!("requirements stage panicked")))?;
    stages.requirements = StageStatus::Pass;
    if let Some(diag) = requirements.diagnostic {
        diagnostics.push(diag);
    }

    let setup_result = AssertUnwindSafe(run_setup(&pack, &requirements.state, &providers, deps))
        .catch_unwind()
        .await
        .unwrap_or_else(|_| Err(anyhow!("setup stage panicked")));
    match setup_result {
        Ok(summary) => {
            diagnostics.push(E2eDiagnostic {
                stage: Some("setup".into()),
                message: format!(
                    "plan=config_patches={} secrets_patches={} subscriptions={} webhooks={}",
                    summary.config_patches,
                    summary.secrets_patches,
                    summary.subscriptions,
                    summary.webhooks
                ),
            });
            stages.setup = StageStatus::Pass;
        }
        Err(err) => {
            diagnostics.push(E2eDiagnostic {
                stage: Some("setup".into()),
                message: err.to_string(),
            });
            stages.setup = StageStatus::Fail;
        }
    }

    if has_source || has_bridge {
        let ingest = AssertUnwindSafe(run_ingest(&pack, &providers, deps, options))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Err(anyhow!("ingest stage panicked")));
        match ingest {
            Ok(_) => stages.ingest = StageStatus::Pass,
            Err(err) => {
                diagnostics.push(E2eDiagnostic {
                    stage: Some("ingest".into()),
                    message: err.to_string(),
                });
                stages.ingest = StageStatus::Fail;
            }
        }
    }

    if has_sink || has_bridge {
        let emit = AssertUnwindSafe(run_emit(&pack, &providers, deps, options))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Err(anyhow!("emit stage panicked")));
        match emit {
            Ok(_) => stages.emit = StageStatus::Pass,
            Err(err) => {
                diagnostics.push(E2eDiagnostic {
                    stage: Some("emit".into()),
                    message: err.to_string(),
                });
                stages.emit = StageStatus::Fail;
            }
        }
    }

    if has_broker || requirements.state.subscriptions_required {
        let subscriptions = AssertUnwindSafe(run_subscriptions(&pack, &providers, deps, options))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Err(anyhow!("subscriptions stage panicked")));
        match subscriptions {
            Ok(_) => stages.subscriptions = StageStatus::Pass,
            Err(err) => {
                diagnostics.push(E2eDiagnostic {
                    stage: Some("subscriptions".into()),
                    message: err.to_string(),
                });
                stages.subscriptions = StageStatus::Fail;
            }
        }
    }

    let status = overall_status(&stages);
    Ok(PackReport {
        pack: pack_id,
        version,
        status,
        stages,
        diagnostics,
        timing_ms: TimingMs {
            total: started.elapsed().as_millis() as u64,
        },
    })
}

struct RequirementsResult {
    state: RequirementsFixture,
    diagnostic: Option<E2eDiagnostic>,
}

async fn run_requirements(
    pack: &PackLoad,
    flow_id: Option<&str>,
    deps: &E2eDependencies,
) -> Result<RequirementsResult> {
    let fixture = deps.requirements_runner.run(pack, flow_id).await?;
    validate_requirements_fixture(&fixture)?;
    let mut message = format!(
        "config_keys={:?} secret_keys={:?} subscriptions_required={} webhooks_required={}",
        fixture.required_config_keys,
        fixture.required_secret_keys,
        fixture.subscriptions_required,
        fixture.webhooks_required
    );
    if flow_id.is_some() {
        message = format!("flow={:?} {}", flow_id, message);
    }
    let diagnostic = Some(E2eDiagnostic {
        stage: Some("requirements".into()),
        message,
    });
    Ok(RequirementsResult {
        state: fixture,
        diagnostic,
    })
}

async fn run_setup(
    pack: &PackLoad,
    requirements: &RequirementsFixture,
    providers: &[EventProviderSpec],
    deps: &E2eDependencies,
) -> Result<SetupSummary> {
    let setup: Value = load_fixture(pack, "fixtures/setup.input.json")
        .context("setup fixture missing or invalid")?;
    if !setup.is_object() {
        return Err(anyhow!("setup.input.json must be a JSON object"));
    }

    let mut summary = SetupSummary {
        config_patches: 0,
        secrets_patches: 0,
        subscriptions: 0,
        webhooks: 0,
    };

    for provider in providers {
        let inputs = ProvisionInputs {
            provider_id: provider.name.clone(),
            install_id: "fixture-install".into(),
            public_base_url: None,
            answers: setup.clone(),
            requirements: requirements.clone(),
        };

        let plan = deps.provision_runner.plan(pack, &inputs).await?;
        let plan_again = deps.provision_runner.plan(pack, &inputs).await?;
        if plan != plan_again {
            return Err(anyhow!("provisioning output is not deterministic"));
        }

        if requirements.subscriptions_required && plan.subscriptions.is_empty() {
            return Err(anyhow!(
                "subscriptions required but provisioning plan returned no ops"
            ));
        }
        if requirements.webhooks_required && plan.webhooks.is_empty() {
            return Err(anyhow!(
                "webhooks required but provisioning plan returned no ops"
            ));
        }

        summary.config_patches += usize::from(plan.config_patch.is_some());
        summary.secrets_patches += usize::from(plan.secrets_patch.is_some());
        summary.subscriptions += plan.subscriptions.len();
        summary.webhooks += plan.webhooks.len();
    }

    Ok(summary)
}

async fn run_ingest(
    pack: &PackLoad,
    providers: &[EventProviderSpec],
    deps: &E2eDependencies,
    options: &E2eOptions,
) -> Result<()> {
    let events = deps
        .ingest_runner
        .ingest(pack, providers, options.live)
        .await?;
    if events.is_empty() {
        return Err(anyhow!("ingest produced no events"));
    }
    for event in events {
        if event.topic.trim().is_empty() {
            return Err(anyhow!("ingest event missing topic"));
        }
        if event.r#type.trim().is_empty() {
            return Err(anyhow!("ingest event missing type"));
        }
        if event
            .subject
            .as_ref()
            .map(|s| s.trim().is_empty())
            .unwrap_or(true)
        {
            return Err(anyhow!("ingest event missing subject"));
        }
        if event.tenant.tenant.as_str().trim().is_empty() {
            return Err(anyhow!("ingest event missing tenant"));
        }
    }
    Ok(())
}

async fn run_emit(
    pack: &PackLoad,
    providers: &[EventProviderSpec],
    deps: &E2eDependencies,
    options: &E2eOptions,
) -> Result<()> {
    let messages = deps.emit_runner.emit(pack, providers, options.live).await?;
    if messages.is_empty() {
        return Err(anyhow!("emit produced no messages"));
    }
    for msg in messages {
        if msg.channel.trim().is_empty() {
            return Err(anyhow!("emit message missing channel"));
        }
        if msg.tenant.tenant.as_str().trim().is_empty() {
            return Err(anyhow!("emit message missing tenant"));
        }
    }
    Ok(())
}

async fn run_subscriptions(
    pack: &PackLoad,
    providers: &[EventProviderSpec],
    deps: &E2eDependencies,
    options: &E2eOptions,
) -> Result<()> {
    deps.subscription_runner
        .sync(pack, providers, options.live)
        .await
}

async fn registration_from_spec(
    spec: &EventProviderSpec,
    factory: &DryRunProviderFactory,
) -> Result<ProviderRegistration> {
    let instance = factory.build_provider(spec).await?;
    let descriptor = descriptor_from_spec(spec);
    Ok(ProviderRegistration {
        descriptor,
        topics: spec.capabilities.topics.clone(),
        dlq_topic: None,
        retry: Default::default(),
        instance,
    })
}

fn load_fixture<T>(pack: &PackLoad, path: &str) -> Result<T>
where
    T: for<'de> Deserialize<'de>,
{
    let bytes = pack
        .files
        .get(path)
        .ok_or_else(|| anyhow!("missing fixture {}", path))?;
    serde_json::from_slice(bytes).with_context(|| format!("parse fixture {}", path))
}

fn report_to_pack_report(pack_path: &Path, result: Result<PackReport>) -> PackReport {
    match result {
        Ok(report) => report,
        Err(err) => PackReport {
            pack: pack_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string(),
            version: "unknown".into(),
            status: StageStatus::Fail,
            stages: StageReport {
                requirements: StageStatus::Fail,
                setup: StageStatus::Fail,
                ingest: StageStatus::Fail,
                emit: StageStatus::Fail,
                subscriptions: StageStatus::Fail,
            },
            diagnostics: vec![E2eDiagnostic {
                stage: None,
                message: err.to_string(),
            }],
            timing_ms: TimingMs { total: 0 },
        },
    }
}

fn overall_status(stages: &StageReport) -> StageStatus {
    let statuses = [
        &stages.requirements,
        &stages.setup,
        &stages.ingest,
        &stages.emit,
        &stages.subscriptions,
    ];
    if statuses
        .iter()
        .any(|status| matches!(status, StageStatus::Fail))
    {
        StageStatus::Fail
    } else if statuses
        .iter()
        .all(|status| matches!(status, StageStatus::Skip))
    {
        StageStatus::Skip
    } else {
        StageStatus::Pass
    }
}

fn summarize(reports: &[PackReport]) -> E2eSummary {
    let mut summary = E2eSummary {
        total: reports.len(),
        pass: 0,
        fail: 0,
        skip: 0,
    };
    for report in reports {
        match report.status {
            StageStatus::Pass => summary.pass += 1,
            StageStatus::Fail => summary.fail += 1,
            StageStatus::Skip => summary.skip += 1,
        }
    }
    summary
}

fn sample_topic(pattern: &str) -> String {
    if pattern.contains('*') {
        pattern.replace('*', "test")
    } else {
        pattern.to_string()
    }
}

fn validate_requirements_fixture(fixture: &RequirementsFixture) -> Result<()> {
    for key in &fixture.required_config_keys {
        if key.trim().is_empty() {
            return Err(anyhow!("requirements fixture has empty config key"));
        }
    }
    for key in &fixture.required_secret_keys {
        if key.trim().is_empty() {
            return Err(anyhow!("requirements fixture has empty secret key"));
        }
    }
    Ok(())
}

fn requirements_flow_id(pack: &PackLoad) -> Option<String> {
    if let Some(flow) = pack
        .manifest
        .meta
        .entry_flows
        .iter()
        .find(|flow| flow.contains("requirements"))
    {
        return Some(flow.clone());
    }
    pack.manifest
        .flows
        .iter()
        .map(|flow| flow.id.as_str())
        .find(|flow| flow.contains("requirements"))
        .map(|flow| flow.to_string())
}

fn apply_dry_run_guard(options: &E2eOptions) -> Result<()> {
    if options.dry_run {
        let _ = std::env::var("GREENTIC_EVENTS_DRY_RUN");
        let _ = std::env::var("GREENTIC_NETWORK_DISABLED");
        let _ = std::env::var("GREENTIC_FILESYSTEM_DISABLED");
        let _ = std::env::var("GREENTIC_RANDOM_SEED");
        let _ = std::env::var("GREENTIC_FIXED_TIME");
        if options.report_path.is_some()
            && std::env::var("GREENTIC_EVENTS_E2E_ALLOW_WRITE").unwrap_or_default() != "1"
        {
            return Err(anyhow!(
                "filesystem writes are disabled in dry-run (set GREENTIC_EVENTS_E2E_ALLOW_WRITE=1 to allow report output)"
            ));
        }
    }
    Ok(())
}

fn setup_flow_id(pack: &PackLoad) -> Option<String> {
    if let Some(flow) = pack
        .manifest
        .meta
        .entry_flows
        .iter()
        .find(|flow| flow.contains("setup") || flow.contains("provision"))
    {
        return Some(flow.clone());
    }
    pack.manifest
        .flows
        .iter()
        .map(|flow| flow.id.as_str())
        .find(|flow| flow.contains("setup") || flow.contains("provision"))
        .map(|flow| flow.to_string())
}

fn run_config_flow_from_pack(
    pack: &PackLoad,
    flow_id: &str,
    answers: Option<&Value>,
) -> Result<Option<Value>> {
    let entry = match pack.manifest.flows.iter().find(|flow| flow.id == flow_id) {
        Some(entry) => entry,
        None => return Ok(None),
    };
    let yaml_bytes = pack
        .files
        .get(&entry.file_yaml)
        .ok_or_else(|| anyhow!("missing flow file {}", entry.file_yaml))?;
    let yaml = std::str::from_utf8(yaml_bytes)
        .with_context(|| format!("flow yaml {} invalid utf-8", entry.file_yaml))?;
    let normalized = normalize_config_flow_yaml(yaml)?;
    let doc = flow_loader::load_ygtc_from_str(&normalized)
        .with_context(|| format!("load flow {}", flow_id))?;
    let flow = compile_flow(doc.clone()).with_context(|| format!("compile flow {}", flow_id))?;

    let mut state = serde_json::Map::new();
    if let Some(Value::Object(map)) = answers {
        state = map.clone();
    }
    let renderer = TemplateRenderer::new(Some(pack.manifest.meta.pack_id.clone()));

    let mut current = resolve_entry(&doc);
    let mut visited = 0usize;
    while visited < flow.nodes.len().saturating_add(4) {
        visited += 1;
        let node_id = greentic_types::NodeId::new(current.as_str())
            .map_err(|e| anyhow!("invalid node id {}: {}", current, e))?;
        let node = flow
            .nodes
            .get(&node_id)
            .ok_or_else(|| anyhow!("node '{}' missing during config flow execution", current))?;

        match node.component.id.as_str() {
            "questions" => {
                apply_questions(&node.input.mapping, &mut state)?;
            }
            "template" => {
                let payload = render_template(&node.input.mapping, &state, &renderer, &current)?;
                return Ok(Some(payload));
            }
            other => {
                return Err(anyhow!("unsupported component '{}' in config flow", other));
            }
        }

        current = match &node.routing {
            greentic_types::Routing::Next { node_id } => node_id.as_str().to_string(),
            greentic_types::Routing::End | greentic_types::Routing::Reply => {
                return Err(anyhow!(
                    "config flow terminated without reaching template node"
                ));
            }
            greentic_types::Routing::Branch { .. } | greentic_types::Routing::Custom(_) => {
                return Err(anyhow!("unsupported routing shape in config flow"));
            }
        };
    }

    Err(anyhow!("config flow exceeded traversal limit"))
}

fn resolve_entry(doc: &greentic_flow::model::FlowDoc) -> String {
    if let Some(start) = &doc.start {
        return start.clone();
    }
    if doc.nodes.contains_key("in") {
        return "in".to_string();
    }
    doc.nodes
        .keys()
        .next()
        .cloned()
        .unwrap_or_else(|| "in".to_string())
}

fn apply_questions(payload: &Value, state: &mut serde_json::Map<String, Value>) -> Result<()> {
    let fields = payload
        .get("fields")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("questions node missing fields array"))?;

    for field in fields {
        let id = field
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("questions field missing id"))?;
        if state.contains_key(id) {
            continue;
        }
        if let Some(default) = field.get("default") {
            state.insert(id.to_string(), default.clone());
        } else {
            return Err(anyhow!("missing answer for '{}'", id));
        }
    }
    Ok(())
}

fn render_template(
    payload: &Value,
    state: &serde_json::Map<String, Value>,
    renderer: &TemplateRenderer,
    node_id: &str,
) -> Result<Value> {
    let template_str = payload
        .as_str()
        .ok_or_else(|| anyhow!("template node payload must be a string"))?;
    let value = renderer.render_json(template_str, state, node_id)?;
    extract_config_output(value)
}

fn extract_config_output(value: Value) -> Result<Value> {
    let node_id = value
        .get("node_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("config flow output missing node_id"))?;
    if node_id.trim().is_empty() {
        return Err(anyhow!("config flow output node_id is empty"));
    }
    value
        .get("node")
        .cloned()
        .ok_or_else(|| anyhow!("config flow output missing node"))
}

fn normalize_config_flow_yaml(yaml: &str) -> Result<String> {
    let mut value: Value =
        serde_yaml_bw::from_str(yaml).map_err(|e| anyhow!("config flow yaml: {}", e))?;
    if let Some(map) = value.as_object_mut() {
        match map.get("type") {
            Some(Value::String(_)) => {}
            _ => {
                map.insert(
                    "type".to_string(),
                    Value::String("component-config".to_string()),
                );
            }
        }
    }
    serde_yaml_bw::to_string(&value).map_err(|e| anyhow!("normalize config flow: {}", e))
}

fn parse_requirements_from_output(output: &Value) -> Result<RequirementsFixture> {
    serde_json::from_value(output.clone())
        .context("parse requirements flow output as RequirementsFixture")
}

fn parse_provision_plan_from_output(output: &Value) -> Result<ProvisionPlan> {
    serde_json::from_value(output.clone()).context("parse provision flow output as ProvisionPlan")
}

fn env_command(var: &str) -> Result<String> {
    let value = std::env::var(var).unwrap_or_default();
    if value.trim().is_empty() {
        return Err(anyhow!("{} must be set for live mode", var));
    }
    Ok(value)
}

fn run_command_json(command: &str, payload: &Value) -> Result<Value> {
    let mut parts = command.split_whitespace();
    let exe = parts.next().ok_or_else(|| anyhow!("command is empty"))?;
    let mut cmd = Command::new(exe);
    for arg in parts {
        cmd.arg(arg);
    }
    let output = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let data = serde_json::to_vec(payload)?;
                stdin.write_all(&data)?;
            }
            child.wait_with_output()
        })
        .with_context(|| format!("run live command: {}", command))?;
    if !output.status.success() {
        return Err(anyhow!(
            "live command failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout = String::from_utf8(output.stdout).context("live command stdout not utf-8")?;
    serde_json::from_str(&stdout).context("parse live command response json")
}

async fn command_ingest(
    command: String,
    pack: &PackLoad,
    providers: &[EventProviderSpec],
) -> Result<Vec<EventEnvelope>> {
    let message: ChannelMessageEnvelope = load_fixture(pack, "fixtures/ingest.request.json")
        .context("ingest fixture missing or invalid")?;
    let provider_name = providers
        .iter()
        .find(|provider| {
            matches!(
                provider.kind,
                EventProviderKind::Source | EventProviderKind::Bridge
            )
        })
        .map(|provider| provider.name.clone())
        .unwrap_or_else(|| "events".to_string());
    let payload = serde_json::json!({
        "op": "ingest",
        "pack_id": pack.manifest.meta.pack_id,
        "provider": provider_name,
        "message": message,
    });
    let response = run_command_json(&command, &payload)?;
    let events = response
        .get("events")
        .cloned()
        .ok_or_else(|| anyhow!("live ingest response missing events"))?;
    serde_json::from_value(events).context("parse live ingest events")
}

async fn command_emit(
    command: String,
    pack: &PackLoad,
    providers: &[EventProviderSpec],
) -> Result<Vec<ChannelMessageEnvelope>> {
    let event: EventEnvelope = load_fixture(pack, "fixtures/emit.request.json")
        .context("emit fixture missing or invalid")?;
    let provider_name = providers
        .iter()
        .find(|provider| {
            matches!(
                provider.kind,
                EventProviderKind::Sink | EventProviderKind::Bridge
            )
        })
        .map(|provider| provider.name.clone())
        .unwrap_or_else(|| "events".to_string());
    let payload = serde_json::json!({
        "op": "emit",
        "pack_id": pack.manifest.meta.pack_id,
        "provider": provider_name,
        "event": event,
    });
    let response = run_command_json(&command, &payload)?;
    let messages = response
        .get("messages")
        .cloned()
        .ok_or_else(|| anyhow!("live emit response missing messages"))?;
    serde_json::from_value(messages).context("parse live emit messages")
}

async fn command_subscriptions(
    command: String,
    pack: &PackLoad,
    providers: &[EventProviderSpec],
) -> Result<()> {
    let expected: Value = load_fixture(pack, "fixtures/subscriptions.expected.json")
        .context("subscriptions fixture missing or invalid")?;
    if !expected.is_array() {
        return Err(anyhow!(
            "subscriptions expected fixture must be a JSON array"
        ));
    }
    let broker_provider = providers
        .iter()
        .find(|provider| matches!(provider.kind, EventProviderKind::Broker))
        .ok_or_else(|| anyhow!("subscriptions requested but no broker providers declared"))?;

    let topic = match broker_provider.capabilities.topics.first() {
        Some(pattern) => sample_topic(pattern),
        None => "greentic.events.test".to_string(),
    };

    let tenant = TenantCtx::new(
        "dev".try_into().map_err(|_| anyhow!("invalid env id"))?,
        "tenant"
            .try_into()
            .map_err(|_| anyhow!("invalid tenant id"))?,
    );
    let options = SubscriptionOptions {
        durable: false,
        deliver_existing: false,
        ack_mode: AckMode::Auto,
    };

    let subscribe_payload = serde_json::json!({
        "op": "subscribe",
        "pack_id": pack.manifest.meta.pack_id,
        "provider": broker_provider.name,
        "topic": topic,
        "options": {
            "durable": options.durable,
            "deliver_existing": options.deliver_existing,
            "ack_mode": match options.ack_mode { AckMode::Auto => "auto", AckMode::Manual => "manual" }
        }
    });
    let response = run_command_json(&command, &subscribe_payload)?;
    let subscription_id = response
        .get("subscription_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("live subscribe response missing subscription_id"))?;

    let time = DateTime::parse_from_rfc3339(FIXED_TIME)
        .map_err(|err| anyhow!(err))?
        .with_timezone(&Utc);
    let event = EventEnvelope {
        id: EventId::new("evt-subscription")?,
        topic: topic.clone(),
        r#type: "greentic.events.subscription".into(),
        source: "greentic-events-test".into(),
        tenant,
        subject: Some("subscription".into()),
        time,
        correlation_id: None,
        payload: Value::Object(Default::default()),
        metadata: Default::default(),
    };

    let publish_payload = serde_json::json!({
        "op": "publish",
        "pack_id": pack.manifest.meta.pack_id,
        "provider": broker_provider.name,
        "event": event,
    });
    run_command_json(&command, &publish_payload)?;

    let next_payload = serde_json::json!({
        "op": "next_event",
        "pack_id": pack.manifest.meta.pack_id,
        "provider": broker_provider.name,
        "subscription_id": subscription_id,
        "timeout_ms": 500
    });
    let response = run_command_json(&command, &next_payload)?;
    let event_value = response
        .get("event")
        .cloned()
        .ok_or_else(|| anyhow!("live next_event response missing event"))?;
    let received: EventEnvelope =
        serde_json::from_value(event_value).context("parse live event")?;
    if received.topic.trim().is_empty() {
        return Err(anyhow!("subscription received empty topic"));
    }

    if options.ack_mode == AckMode::Auto {
        let ack_payload = serde_json::json!({
            "op": "ack_event",
            "pack_id": pack.manifest.meta.pack_id,
            "provider": broker_provider.name,
            "subscription_id": subscription_id,
            "event_id": received.id.to_string(),
        });
        run_command_json(&command, &ack_payload)?;
    }
    Ok(())
}
