wit_bindgen::generate!({
    path: "wit/greentic/pack-validate@0.1.0/package.wit",
    world: "pack-validator",
});

use greentic_types::pack_manifest::{ExtensionInline, PackManifest};
use greentic_types::provider::PROVIDER_EXTENSION_ID;
use serde_json::Value;

use exports::greentic::pack_validate::validator::{Diagnostic, Guest, PackInputs};

const TOPIC_ANNOTATION_KEYS: [&str; 2] = ["event.topic", "events.topic"];

struct EventsPackValidator;

impl Guest for EventsPackValidator {
    fn applies(inputs: PackInputs) -> bool {
        let Some(manifest) = decode_manifest(&inputs.manifest_cbor) else {
            return false;
        };
        is_events_pack(&manifest, &inputs.file_index)
    }

    fn validate(inputs: PackInputs) -> Vec<Diagnostic> {
        let Some(manifest) = decode_manifest(&inputs.manifest_cbor) else {
            return Vec::new();
        };
        if !is_events_pack(&manifest, &inputs.file_index) {
            return Vec::new();
        }

        let mut diagnostics = Vec::new();
        diagnostics.extend(validate_schema_refs(&manifest, &inputs.file_index));
        diagnostics.extend(validate_topics(&manifest));
        diagnostics.extend(validate_entry_flows(&manifest));
        diagnostics
    }
}

export!(EventsPackValidator);

fn decode_manifest(bytes: &[u8]) -> Option<PackManifest> {
    greentic_types::decode_pack_manifest(bytes).ok()
}

fn is_events_pack(manifest: &PackManifest, file_index: &[String]) -> bool {
    if manifest.pack_id.as_str().starts_with("events-") {
        return true;
    }

    if file_index
        .iter()
        .any(|path| path.contains("schemas/events/"))
    {
        return true;
    }

    if let Some(provider_inline) = manifest.provider_extension_inline() {
        for provider in &provider_inline.providers {
            if provider.provider_type.starts_with("events.") {
                return true;
            }
        }
    }

    false
}

fn validate_schema_refs(manifest: &PackManifest, file_index: &[String]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let Some(provider_inline) = manifest.provider_extension_inline() else {
        return diagnostics;
    };

    for provider in &provider_inline.providers {
        if let Some(schema_path) = extract_schema_path(&provider.config_schema_ref)
            && !file_index_contains(file_index, schema_path)
        {
            diagnostics.push(diagnostic(
                "error",
                "EVT_SCHEMA_MISSING",
                format!(
                    "Schema reference '{}' for provider '{}' is missing from the pack.",
                    schema_path, provider.provider_type
                ),
                Some(format!(
                    "extensions.{}.providers.{}.config_schema_ref",
                    PROVIDER_EXTENSION_ID, provider.provider_type
                )),
                Some("Add the schema file to the pack or update the reference.".to_owned()),
            ));
        }

        if let Some(state_ref) = provider.state_schema_ref.as_deref()
            && let Some(schema_path) = extract_schema_path(state_ref)
            && !file_index_contains(file_index, schema_path)
        {
            diagnostics.push(diagnostic(
                "error",
                "EVT_SCHEMA_MISSING",
                format!(
                    "Schema reference '{}' for provider '{}' is missing from the pack.",
                    schema_path, provider.provider_type
                ),
                Some(format!(
                    "extensions.{}.providers.{}.state_schema_ref",
                    PROVIDER_EXTENSION_ID, provider.provider_type
                )),
                Some("Add the schema file to the pack or update the reference.".to_owned()),
            ));
        }
    }

    diagnostics
}

fn validate_topics(manifest: &PackManifest) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let topic = manifest_annotation_value(manifest, &TOPIC_ANNOTATION_KEYS);

    if let Some(value) = topic
        && !is_valid_event_topic(value)
    {
        diagnostics.push(diagnostic(
            "error",
            "EVT_INVALID_TOPIC",
            format!(
                "Event topic '{}' must match ^[a-z0-9]+(\\.[a-z0-9_-]+)+$.",
                value
            ),
            Some("extensions.*.annotations.event.topic".to_owned()),
            Some("Use lowercase dot-separated segments, e.g. 'events.order.created'.".to_owned()),
        ));
    }

    diagnostics
}

fn validate_entry_flows(manifest: &PackManifest) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let entry_flows = manifest_entry_flows(manifest);

    if entry_flows.is_empty() {
        return diagnostics;
    }

    let flow_ids: std::collections::BTreeSet<String> = manifest
        .flows
        .iter()
        .map(|entry| entry.id.as_str().to_owned())
        .collect();

    for flow_id in &entry_flows {
        if !flow_ids.contains(flow_id) {
            diagnostics.push(diagnostic(
                "error",
                "EVT_ENTRY_FLOW_MISSING",
                format!("Entry flow '{}' is missing from manifest.flows.", flow_id),
                Some("meta.entry_flows".to_owned()),
                Some("Add the flow entry or update the entry_flows list.".to_owned()),
            ));
        }

        if flow_id.starts_with("handle_") && !flow_ids.contains(flow_id) {
            diagnostics.push(diagnostic(
                "warn",
                "EVT_HANDLER_FLOW_MISSING",
                format!("Handler flow '{}' is missing from manifest.flows.", flow_id),
                Some(format!("flows.{}", flow_id)),
                Some("Add the handler flow entry or update the entry flow list.".to_owned()),
            ));
        }
    }

    diagnostics
}

fn manifest_entry_flows(manifest: &PackManifest) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
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
    seen: &mut std::collections::BTreeSet<String>,
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

fn extract_schema_path(value: &str) -> Option<&str> {
    value.find("schemas/").map(|idx| &value[idx..])
}

fn file_index_contains(file_index: &[String], path: &str) -> bool {
    file_index.iter().any(|entry| entry == path)
}

fn diagnostic(
    severity: &str,
    code: &str,
    message: impl Into<String>,
    path: Option<String>,
    hint: Option<String>,
) -> Diagnostic {
    Diagnostic {
        severity: severity.to_owned(),
        code: code.to_owned(),
        message: message.into(),
        path,
        hint,
    }
}
