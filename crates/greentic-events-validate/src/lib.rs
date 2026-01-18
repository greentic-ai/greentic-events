use std::collections::{BTreeSet, HashMap};

use greentic_types::pack_manifest::{ExtensionInline, PackManifest};
use greentic_types::provider::PROVIDER_EXTENSION_ID;
use greentic_types::validate::{Diagnostic, PackValidator, Severity};
use serde_json::Value;

const TOPIC_ANNOTATION_KEYS: [&str; 2] = ["event.topic", "events.topic"];
const TYPE_ANNOTATION_KEYS: [&str; 2] = ["event.type", "events.type"];

pub fn events_validators() -> Vec<Box<dyn PackValidator>> {
    vec![
        Box::new(EventsProviderDeclValidator),
        Box::new(EventsTopicTypeFormatValidator),
        Box::new(EventsHandlerFlowValidator),
    ]
}

fn is_events_pack(manifest: &PackManifest) -> bool {
    if manifest.pack_id.as_str().starts_with("events-") {
        return true;
    }

    if let Some(provider_inline) = manifest.provider_extension_inline() {
        for provider in &provider_inline.providers {
            if provider.provider_type.starts_with("events.") {
                return true;
            }
            if schema_ref_contains_events(&provider.config_schema_ref) {
                return true;
            }
            if let Some(state_ref) = provider.state_schema_ref.as_deref()
                && schema_ref_contains_events(state_ref)
            {
                return true;
            }
        }
    }

    false
}

fn schema_ref_contains_events(value: &str) -> bool {
    value.contains("schemas/events/")
}

fn diagnostic(
    severity: Severity,
    code: &str,
    message: impl Into<String>,
    path: Option<String>,
    hint: Option<String>,
) -> Diagnostic {
    Diagnostic {
        severity,
        code: code.to_owned(),
        message: message.into(),
        path,
        hint,
        data: Value::Null,
    }
}

fn manifest_entry_flows(manifest: &PackManifest) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut entries = Vec::new();

    if let Some(extensions) = &manifest.extensions {
        for extension in extensions.values() {
            let Some(ExtensionInline::Other(value)) = extension.inline.as_ref() else {
                continue;
            };

            if let Some(meta) = value.get("meta").and_then(Value::as_object) {
                collect_entry_flows(meta.get("entry_flows"), &mut seen, &mut entries);
            }
            collect_entry_flows(value.get("entry_flows"), &mut seen, &mut entries);
        }
    }

    entries
}

fn collect_entry_flows(
    value: Option<&Value>,
    seen: &mut BTreeSet<String>,
    entries: &mut Vec<String>,
) {
    let Some(Value::Array(items)) = value else {
        return;
    };

    for item in items {
        if let Some(flow_id) = item.as_str()
            && seen.insert(flow_id.to_owned())
        {
            entries.push(flow_id.to_owned());
        }
    }
}

fn manifest_annotation_value<'a>(manifest: &'a PackManifest, keys: &[&str]) -> Option<&'a str> {
    let extensions = manifest.extensions.as_ref()?;
    for extension in extensions.values() {
        let Some(ExtensionInline::Other(value)) = extension.inline.as_ref() else {
            continue;
        };
        let Some(annotations) = value.get("annotations").and_then(Value::as_object) else {
            continue;
        };
        for key in keys {
            if let Some(value) = annotations.get(*key).and_then(Value::as_str) {
                return Some(value);
            }
        }
    }
    None
}

fn is_valid_event_topic(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    let mut segments = value.split('.');
    let Some(first) = segments.next() else {
        return false;
    };
    if first.is_empty()
        || !first
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    {
        return false;
    }
    let mut has_tail = false;
    for segment in segments {
        has_tail = true;
        if segment.is_empty()
            || !segment
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return false;
        }
    }
    has_tail
}

fn is_reverse_dns(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    value.starts_with("com.") || value.starts_with("org.") || value.starts_with("ai.greentic.")
}

struct EventsProviderDeclValidator;

impl PackValidator for EventsProviderDeclValidator {
    fn id(&self) -> &'static str {
        "events.provider-decl"
    }

    fn applies(&self, manifest: &PackManifest) -> bool {
        is_events_pack(manifest)
    }

    fn validate(&self, manifest: &PackManifest) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let Some(provider_inline) = manifest.provider_extension_inline() else {
            return diagnostics;
        };

        for provider in &provider_inline.providers {
            if provider.config_schema_ref.trim().is_empty() {
                diagnostics.push(diagnostic(
                    Severity::Error,
                    "EVT_PROVIDER_SCHEMA_EMPTY",
                    format!(
                        "Provider '{}' must declare a non-empty config schema reference.",
                        provider.provider_type
                    ),
                    Some(format!(
                        "extensions.{}.providers.{}.config_schema_ref",
                        PROVIDER_EXTENSION_ID, provider.provider_type
                    )),
                    Some("Set config_schema_ref to a schema path or URL.".to_owned()),
                ));
            }

            if provider.ops.is_empty() {
                diagnostics.push(diagnostic(
                    Severity::Warn,
                    "EVT_PROVIDER_NO_OPS",
                    format!(
                        "Provider '{}' declares no operations.",
                        provider.provider_type
                    ),
                    Some(format!(
                        "extensions.{}.providers.{}.ops",
                        PROVIDER_EXTENSION_ID, provider.provider_type
                    )),
                    Some(
                        "Declare at least one operation when the provider is implemented."
                            .to_owned(),
                    ),
                ));
            }
        }

        diagnostics
    }
}

struct EventsTopicTypeFormatValidator;

impl PackValidator for EventsTopicTypeFormatValidator {
    fn id(&self) -> &'static str {
        "events.topic-type-format"
    }

    fn applies(&self, manifest: &PackManifest) -> bool {
        is_events_pack(manifest)
    }

    fn validate(&self, manifest: &PackManifest) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        let topic = manifest_annotation_value(manifest, &TOPIC_ANNOTATION_KEYS);
        let event_type = manifest_annotation_value(manifest, &TYPE_ANNOTATION_KEYS);

        if let Some(value) = topic
            && !is_valid_event_topic(value)
        {
            diagnostics.push(diagnostic(
                Severity::Error,
                "EVT_INVALID_TOPIC",
                format!(
                    "Event topic '{}' must match ^[a-z0-9]+(\\.[a-z0-9_-]+)+$.",
                    value
                ),
                Some("extensions.*.annotations.event.topic".to_owned()),
                Some(
                    "Use lowercase dot-separated segments, e.g. 'events.order.created'.".to_owned(),
                ),
            ));
        }

        if let Some(value) = event_type
            && (value.trim().is_empty() || !is_reverse_dns(value))
        {
            diagnostics.push(diagnostic(
                Severity::Warn,
                "EVT_SUSPECT_TYPE_FORMAT",
                format!(
                    "Event type '{}' should be non-empty and reverse-DNS (com., org., ai.greentic.).",
                    value
                ),
                Some("extensions.*.annotations.event.type".to_owned()),
                Some("Prefer reverse-DNS identifiers such as 'com.example.events.created'.".to_owned()),
            ));
        }

        diagnostics
    }
}

struct EventsHandlerFlowValidator;

impl PackValidator for EventsHandlerFlowValidator {
    fn id(&self) -> &'static str {
        "events.handler-flow"
    }

    fn applies(&self, manifest: &PackManifest) -> bool {
        if !is_events_pack(manifest) {
            return false;
        }
        let entry_flows = manifest_entry_flows(manifest);
        if entry_flows.iter().any(|flow| {
            flow.contains("events") || flow.starts_with("handle_") || flow.starts_with("on_")
        }) {
            return true;
        }
        manifest.flows.iter().any(|entry| {
            entry.id.as_str().starts_with("handle_") || entry.id.as_str().starts_with("on_")
        })
    }

    fn validate(&self, manifest: &PackManifest) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        let entry_flows = manifest_entry_flows(manifest);

        let mut flow_index: HashMap<String, &greentic_types::pack_manifest::PackFlowEntry> =
            HashMap::new();
        for entry in &manifest.flows {
            flow_index.insert(entry.id.as_str().to_owned(), entry);
        }

        for flow_id in &entry_flows {
            if !flow_index.contains_key(flow_id) {
                diagnostics.push(diagnostic(
                    Severity::Error,
                    "EVT_ENTRY_FLOW_MISSING",
                    format!("Entry flow '{}' is missing from manifest.flows.", flow_id),
                    Some("meta.entry_flows".to_owned()),
                    Some("Add the flow entry or update the entry_flows list.".to_owned()),
                ));
            }

            if flow_id.starts_with("handle_") {
                match flow_index.get(flow_id) {
                    Some(entry) => {
                        if entry.flow.schema_version != "flow-v1" {
                            diagnostics.push(diagnostic(
                                Severity::Error,
                                "EVT_HANDLER_FLOW_MISSING",
                                format!("Handler flow '{}' must be a flow-v1 schema.", flow_id),
                                Some(format!("flows.{}.schema_version", flow_id)),
                                Some("Ensure the flow schema_version is 'flow-v1'.".to_owned()),
                            ));
                        }
                    }
                    None => {
                        diagnostics.push(diagnostic(
                            Severity::Error,
                            "EVT_HANDLER_FLOW_MISSING",
                            format!("Handler flow '{}' is missing.", flow_id),
                            Some(format!("flows.{}", flow_id)),
                            Some("Add the handler flow entry to the manifest.".to_owned()),
                        ));
                    }
                }
            }
        }

        for entry in &manifest.flows {
            if entry.id.as_str().starts_with("handle_") && entry.flow.schema_version != "flow-v1" {
                diagnostics.push(diagnostic(
                    Severity::Error,
                    "EVT_HANDLER_FLOW_MISSING",
                    format!(
                        "Handler flow '{}' must be a flow-v1 schema.",
                        entry.id.as_str()
                    ),
                    Some(format!("flows.{}.schema_version", entry.id.as_str())),
                    Some("Ensure the flow schema_version is 'flow-v1'.".to_owned()),
                ));
            }
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use greentic_types::flow::{Flow, FlowHasher, FlowKind, FlowMetadata, Node};
    use greentic_types::pack_manifest::{
        ExtensionInline, ExtensionRef, PackFlowEntry, PackKind, PackManifest, PackSignatures,
    };
    use greentic_types::provider::{ProviderDecl, ProviderRuntimeRef};
    use greentic_types::{FlowId, PackId};
    use indexmap::IndexMap;
    use semver::Version;
    use serde_json::json;

    use super::events_validators;

    fn run_validators(manifest: &PackManifest) -> Vec<greentic_types::validate::Diagnostic> {
        let mut diagnostics = Vec::new();
        for validator in events_validators() {
            if validator.applies(manifest) {
                diagnostics.extend(validator.validate(manifest));
            }
        }
        diagnostics
    }

    fn base_manifest(pack_id: &str) -> PackManifest {
        PackManifest {
            schema_version: "greentic.pack-manifest.v1".to_owned(),
            pack_id: PackId::new(pack_id).expect("pack_id"),
            version: Version::new(0, 1, 0),
            kind: PackKind::Application,
            publisher: "greentic".to_owned(),
            components: Vec::new(),
            flows: Vec::new(),
            dependencies: Vec::new(),
            capabilities: Vec::new(),
            secret_requirements: Vec::new(),
            signatures: PackSignatures::default(),
            bootstrap: None,
            extensions: None,
        }
    }

    fn flow_entry(id: &str) -> PackFlowEntry {
        let flow_id = FlowId::new(id).expect("flow_id");
        PackFlowEntry {
            id: flow_id.clone(),
            kind: FlowKind::Event,
            flow: Flow {
                schema_version: "flow-v1".to_owned(),
                id: flow_id,
                kind: FlowKind::Event,
                entrypoints: BTreeMap::new(),
                nodes: IndexMap::<greentic_types::NodeId, Node, FlowHasher>::default(),
                metadata: FlowMetadata::default(),
            },
            tags: Vec::new(),
            entrypoints: Vec::new(),
        }
    }

    fn add_entry_flows(manifest: &mut PackManifest, entry_flows: Vec<&str>) {
        let mut extensions = BTreeMap::new();
        extensions.insert(
            "greentic.events-meta.v1".to_owned(),
            ExtensionRef {
                kind: "greentic.events-meta.v1".to_owned(),
                version: "1.0.0".to_owned(),
                digest: None,
                location: None,
                inline: Some(ExtensionInline::Other(json!({
                    "meta": {
                        "entry_flows": entry_flows,
                    }
                }))),
            },
        );
        manifest.extensions = Some(extensions);
    }

    #[test]
    fn non_events_pack_no_diagnostics() {
        let manifest = base_manifest("demo-pack");
        let diagnostics = run_validators(&manifest);
        assert!(diagnostics.is_empty(), "expected no diagnostics");
    }

    #[test]
    fn entry_flow_missing_emits_error() {
        let mut manifest = base_manifest("events-demo");
        add_entry_flows(&mut manifest, vec!["events.start"]);

        let diagnostics = run_validators(&manifest);
        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.code == "EVT_ENTRY_FLOW_MISSING")
        );
    }

    #[test]
    fn provider_schema_empty_emits_error() {
        let mut manifest = base_manifest("events-demo");
        let provider = ProviderDecl {
            provider_type: "events.fake".to_owned(),
            capabilities: Vec::new(),
            ops: vec!["publish".to_owned()],
            config_schema_ref: "".to_owned(),
            state_schema_ref: None,
            runtime: ProviderRuntimeRef {
                component_ref: "component".to_owned(),
                export: "export".to_owned(),
                world: "world".to_owned(),
            },
            docs_ref: None,
        };
        manifest.ensure_provider_extension_inline().providers = vec![provider];

        let diagnostics = run_validators(&manifest);
        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.code == "EVT_PROVIDER_SCHEMA_EMPTY")
        );
    }

    #[test]
    fn handler_flow_missing_emits_error() {
        let mut manifest = base_manifest("events-demo");
        add_entry_flows(&mut manifest, vec!["handle_event"]);

        let diagnostics = run_validators(&manifest);
        assert!(
            diagnostics
                .iter()
                .any(|diag| diag.code == "EVT_HANDLER_FLOW_MISSING")
        );
    }

    #[test]
    fn handler_flow_present_no_error() {
        let mut manifest = base_manifest("events-demo");
        manifest.flows.push(flow_entry("handle_event"));
        add_entry_flows(&mut manifest, vec!["handle_event"]);

        let diagnostics = run_validators(&manifest);
        assert!(
            !diagnostics
                .iter()
                .any(|diag| diag.code == "EVT_HANDLER_FLOW_MISSING")
        );
    }
}
