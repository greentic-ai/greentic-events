use anyhow::Result;
use chrono::{DateTime, Utc};
use greentic_events::e2e::{E2eOptions, StageStatus, run_e2e};
use greentic_pack::builder::{FlowBundle, PackBuilder, PackMeta};
use greentic_pack::events::{EventProviderCapabilities, EventProviderKind, EventProviderSpec, EventsSection};
use greentic_types::{ChannelMessageEnvelope, EnvId, EventEnvelope, EventId, TenantCtx, TenantId};
use semver::Version;
use serde_json::json;
use tempfile::tempdir;

fn sample_flow() -> FlowBundle {
    let flow_json = json!({
        "id": "main",
        "kind": "flow/v1",
        "entry": "start",
        "nodes": []
    });
    FlowBundle {
        id: "main".to_string(),
        kind: "flow/v1".to_string(),
        entry: "start".to_string(),
        yaml: "id: main\nentry: start\n".to_string(),
        json: flow_json,
        hash_blake3: "dummyhash".to_string(),
        nodes: Vec::new(),
    }
}

#[tokio::test]
async fn e2e_dry_run_passes() -> Result<()> {
    let tmp = tempdir()?;
    let pack_path = tmp.path().join("events-dummy.gtpack");

    let provider = EventProviderSpec {
        name: "dummy".to_string(),
        kind: EventProviderKind::Bridge,
        component: "dummy@0.1.0".to_string(),
        default_flow: None,
        custom_flow: None,
        capabilities: EventProviderCapabilities {
            topics: vec!["greentic.test".to_string()],
            ..Default::default()
        },
    };

    let meta = PackMeta {
        pack_version: 1,
        pack_id: "events-dummy".to_string(),
        version: Version::parse("0.1.0")?,
        name: "Events Dummy".to_string(),
        kind: None,
        description: Some("dummy e2e pack".to_string()),
        authors: vec!["Greentic".to_string()],
        license: Some("MIT".to_string()),
        homepage: None,
        support: None,
        vendor: None,
        imports: Vec::new(),
        entry_flows: vec!["main".to_string()],
        created_at_utc: "2025-01-01T00:00:00Z".to_string(),
        events: Some(EventsSection {
            providers: vec![provider],
        }),
        repo: None,
        messaging: None,
        interfaces: Vec::new(),
        annotations: Default::default(),
        distribution: None,
        components: Vec::new(),
    };

    let tenant = TenantCtx::new(
        EnvId::try_from("dev")?,
        TenantId::try_from("tenant")?,
    );
    let ingest = ChannelMessageEnvelope {
        id: "msg-1".to_string(),
        tenant: tenant.clone(),
        channel: "email".to_string(),
        session_id: "session-1".to_string(),
        reply_scope: None,
        user_id: None,
        correlation_id: Some("corr-1".to_string()),
        text: Some("hello".to_string()),
        attachments: Vec::new(),
        metadata: Default::default(),
    };
    let time = DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")?.with_timezone(&Utc);
    let emit = EventEnvelope {
        id: EventId::new("evt-1")?,
        topic: "greentic.test".to_string(),
        r#type: "greentic.events.test".to_string(),
        source: "greentic-events-test".to_string(),
        tenant,
        subject: Some("subject-1".to_string()),
        time,
        correlation_id: None,
        payload: json!({"hello": "world"}),
        metadata: Default::default(),
    };

    PackBuilder::new(meta)
        .with_flow(sample_flow())
        .with_asset_bytes(
            "fixtures/requirements.expected.json",
            serde_json::to_vec(&json!({
                "required_config_keys": ["api_key"],
                "required_secret_keys": ["token"],
                "subscriptions_required": false,
                "webhooks_required": false
            }))?,
        )
        .with_asset_bytes(
            "fixtures/setup.input.json",
            serde_json::to_vec(&json!({"answers": {}}))?,
        )
        .with_asset_bytes(
            "fixtures/ingest.request.json",
            serde_json::to_vec(&ingest)?,
        )
        .with_asset_bytes("fixtures/emit.request.json", serde_json::to_vec(&emit)?)
        .build(&pack_path)?;

    let report = run_e2e(E2eOptions {
        packs_dir: tmp.path().to_path_buf(),
        provider_filter: None,
        report_path: None,
        dry_run: true,
        live: false,
    })
    .await?;

    assert_eq!(report.summary.total, 1);
    assert_eq!(report.summary.fail, 0);
    assert_eq!(report.packs[0].status, StageStatus::Pass);
    assert_eq!(report.packs[0].stages.ingest, StageStatus::Pass);
    assert_eq!(report.packs[0].stages.emit, StageStatus::Pass);
    Ok(())
}
