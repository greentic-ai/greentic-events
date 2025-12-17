use std::fs;
use std::path::PathBuf;

use greentic_config::{ConfigLayer, ConfigResolver};
use greentic_config_types::{ConfigSource, ProvenancePath};
use greentic_types::EnvId;
use tempfile::tempdir;

fn write_toml(path: &PathBuf, contents: &str) {
    fs::create_dir_all(path.parent().expect("parent dir")).unwrap();
    fs::write(path, contents).unwrap();
}

fn set_env(name: &str, value: impl AsRef<std::ffi::OsStr>) {
    unsafe { std::env::set_var(name, value) };
}

fn remove_env(name: &str) {
    unsafe { std::env::remove_var(name) };
}

#[test]
fn cli_overrides_env_project_user_precedence() {
    let tmp = tempdir().unwrap();
    let project_root = tmp.path().join("project");
    let user_home = tmp.path().join("home");

    let user_config = user_home.join(".config/greentic/config.toml");
    write_toml(
        &user_config,
        r#"
[environment]
env_id = "user"
region = "user-region"
"#,
    );

    let project_config = project_root.join(".greentic/config.toml");
    write_toml(
        &project_config,
        r#"
[environment]
env_id = "project"
region = "project-region"
"#,
    );

    let prev_home = std::env::var_os("HOME");
    let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");

    set_env("HOME", &user_home);
    set_env("XDG_CONFIG_HOME", user_home.join(".config"));
    set_env("GREENTIC_ENVIRONMENT_ENV_ID", "env");
    set_env("GREENTIC_ENVIRONMENT_REGION", "env-region");

    let mut cli_layer = ConfigLayer::default();
    cli_layer
        .environment
        .get_or_insert_with(Default::default)
        .env_id = Some(EnvId::try_from("cli").unwrap());

    let resolved = ConfigResolver::new()
        .with_project_root(project_root)
        .with_cli_overrides(cli_layer)
        .allow_dev(true)
        .load()
        .expect("resolve config");

    assert_eq!(
        resolved.config.environment.env_id,
        EnvId::try_from("cli").unwrap(),
        "CLI override should win"
    );
    assert_eq!(
        resolved
            .provenance
            .get(&ProvenancePath("environment.env_id".into())),
        Some(&ConfigSource::Cli)
    );

    assert_eq!(
        resolved.config.environment.region.as_deref(),
        Some("env-region"),
        "env should win when CLI layer does not set field"
    );
    assert_eq!(
        resolved
            .provenance
            .get(&ProvenancePath("environment.region".into())),
        Some(&ConfigSource::Environment)
    );

    if let Some(val) = prev_home {
        set_env("HOME", val);
    } else {
        remove_env("HOME");
    }
    if let Some(val) = prev_xdg {
        set_env("XDG_CONFIG_HOME", val);
    } else {
        remove_env("XDG_CONFIG_HOME");
    }
    remove_env("GREENTIC_ENVIRONMENT_ENV_ID");
    remove_env("GREENTIC_ENVIRONMENT_REGION");
}
