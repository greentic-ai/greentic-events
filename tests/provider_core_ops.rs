use std::fs;
use std::path::{Path, PathBuf};

use greentic_events::{PublishInput, PublishOutput, validate_publish_input};
use jsonschema::{Resource, Validator, draft202012};
use serde_json::Value;

fn manifest_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_json(path: impl AsRef<Path>) -> Value {
    let raw = fs::read_to_string(path).expect("read json file");
    serde_json::from_str(&raw).expect("parse json")
}

fn schema_path(relative: &str) -> PathBuf {
    manifest_path().join("schemas").join(relative)
}

fn compile_schema(relative: &str) -> Validator {
    let schema = read_json(schema_path(relative));
    let envelope = read_json(schema_path("events/common/event_envelope.schema.json"));
    draft202012::options()
        .with_resource(
            "https://schemas.greentic.dev/events/common/event_envelope.schema.json",
            Resource::from_contents(envelope),
        )
        .build(&schema)
        .expect("valid schema")
}

#[test]
fn schemas_compile() {
    compile_schema("events/common/event_envelope.schema.json");
    compile_schema("events/ops/publish.input.schema.json");
    compile_schema("events/ops/publish.output.schema.json");
    compile_schema("events/ops/subscribe.input.schema.json");
    compile_schema("events/ops/subscribe.output.schema.json");
    compile_schema("events/ops/ack.input.schema.json");
    compile_schema("events/ops/ack.output.schema.json");
}

#[test]
fn publish_input_fixture_matches_schema_and_validation() {
    let instance = read_json(manifest_path().join("tests/fixtures/publish_input.json"));
    let schema = compile_schema("events/ops/publish.input.schema.json");
    if let Err(err) = schema.validate(&instance) {
        panic!("schema errors: {err}");
    }

    let typed: PublishInput = serde_json::from_value(instance.clone()).expect("typed input");
    validate_publish_input(&typed).expect("validation ok");

    let roundtrip = serde_json::to_value(&typed).expect("serialize");
    assert_eq!(roundtrip, instance);
}

#[test]
fn publish_output_fixture_matches_schema() {
    let instance = read_json(manifest_path().join("tests/fixtures/publish_output.json"));
    let schema = compile_schema("events/ops/publish.output.schema.json");
    if let Err(err) = schema.validate(&instance) {
        panic!("schema errors: {err}");
    }

    let typed: PublishOutput = serde_json::from_value(instance.clone()).expect("typed output");
    let roundtrip = serde_json::to_value(&typed).expect("serialize");
    assert_eq!(roundtrip, instance);
}

#[test]
fn event_envelope_fragment_matches_schema() {
    let fixture = read_json(manifest_path().join("tests/fixtures/publish_input.json"));
    let event = fixture
        .get("event")
        .cloned()
        .expect("event fragment present");
    let schema = compile_schema("events/common/event_envelope.schema.json");
    if let Err(err) = schema.validate(&event) {
        panic!("schema errors: {err}");
    }
}

#[test]
fn topic_mismatch_is_rejected() {
    let mut input: PublishInput = serde_json::from_value(read_json(
        manifest_path().join("tests/fixtures/publish_input.json"),
    ))
    .expect("typed input");
    input.event.topic = Some("other.topic".into());
    let err = validate_publish_input(&input).expect_err("should reject mismatched topic");
    assert!(
        err.messages.iter().any(|msg| msg.contains("event.topic")),
        "missing event.topic validation error: {:?}",
        err.messages
    );
}
