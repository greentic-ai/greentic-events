use std::process::Command;

#[test]
fn cli_lists_providers_from_fixture() {
    if std::env::var("CLI_SMOKE").is_err() {
        eprintln!("CLI_SMOKE not set; skipping CLI smoke test.");
        return;
    }

    let output = Command::new("cargo")
        .args([
            "run",
            "--bin",
            "greentic-events-cli",
            "--",
            "--pack",
            "tests/fixtures/packs/events_fake",
        ])
        .output()
        .expect("failed to run CLI");

    assert!(
        output.status.success(),
        "cli failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
