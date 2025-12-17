use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use greentic_config::{ConfigFileFormat, ConfigLayer, ConfigResolver, ResolvedConfig, explain};
use greentic_config_types::{
    GreenticConfig, TelemetryConfig as HostTelemetryConfig, TelemetryExporterKind,
};
use greentic_events::bus::EventBusBuilder;
use greentic_events::runner::{InMemoryProviderFactory, NoopProvider};
use greentic_telemetry::export::{ExportConfig, ExportMode, Sampling};
use greentic_telemetry::init::{TelemetryConfig, init_telemetry_from_config};

#[derive(Parser, Debug)]
#[command(author, version, about = "Greentic Events pack inspector")]
struct Cli {
    /// Optional path to an explicit config file (treated as highest-precedence CLI layer).
    #[arg(long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Allow dev-only config fields even when env_id is not dev.
    #[arg(long)]
    allow_dev: bool,

    /// Path to a pack directory containing pack.yaml (required unless using a subcommand).
    #[arg(long, value_name = "DIR")]
    pack: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Inspect an events pack and print provider descriptors.
    Inspect {
        /// Path to a pack directory containing pack.yaml.
        #[arg(long, value_name = "DIR")]
        pack: PathBuf,
    },
    /// Show resolved configuration or explain provenance.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigCommand {
    /// Print the resolved Greentic config for this invocation.
    Show {
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Print provenance + warnings for the resolved config.
    Explain {
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let resolved = resolve_config(cli.config.clone(), cli.allow_dev)?;
    init_telemetry_from_greentic(&resolved.config.telemetry)?;

    let command = match cli.command {
        Some(cmd) => cmd,
        None => Command::Inspect {
            pack: cli
                .pack
                .clone()
                .ok_or_else(|| anyhow!("--pack is required unless using a subcommand"))?,
        },
    };

    match command {
        Command::Inspect { pack } => inspect_pack(pack, &resolved.config).await,
        Command::Config { command } => handle_config_command(command, &resolved),
    }
}

async fn inspect_pack(pack: PathBuf, _config: &GreenticConfig) -> Result<()> {
    // Default to in-memory mocks; caller can swap in Runner* factories when available.
    let provider_factory = InMemoryProviderFactory::new(|| Arc::new(NoopProvider::new()));
    // Bridges are not exercised by this CLI; skip registration.

    let bus = EventBusBuilder::new()
        .register_from_pack_dir(&pack, &provider_factory, None)
        .await?
        .build();

    println!("Providers:");
    for desc in bus.list_providers() {
        println!(
            "- {} ({:?}, transport={:?}, reliability={:?}, ordering={:?})",
            desc.name, desc.kind, desc.transport, desc.reliability, desc.ordering
        );
    }

    Ok(())
}

fn handle_config_command(cmd: ConfigCommand, resolved: &ResolvedConfig) -> Result<()> {
    match cmd {
        ConfigCommand::Show { json } => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&resolved.config)
                        .context("serialize config to json")?
                );
            } else {
                println!("{:#?}", resolved.config);
            }
        }
        ConfigCommand::Explain { json } => {
            let report = explain(&resolved.config, &resolved.provenance, &resolved.warnings);
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report.json)
                        .context("serialize explain report to json")?
                );
            } else {
                println!("{}", report.text);
            }
        }
    }

    if !resolved.warnings.is_empty() {
        eprintln!("Warnings:");
        for warning in &resolved.warnings {
            eprintln!("- {warning}");
        }
    }

    Ok(())
}

fn resolve_config(config_path: Option<PathBuf>, allow_dev: bool) -> Result<ResolvedConfig> {
    let mut resolver = ConfigResolver::new().allow_dev(allow_dev);

    if let Some(path) = config_path {
        let layer = parse_config_layer(&path)?;
        resolver = resolver.with_cli_overrides(layer);
        if let Some(project_root) = inferred_project_root_from_config_path(&path) {
            resolver = resolver.with_project_root(project_root);
        }
    }

    resolver.load().context("resolve greentic config")
}

fn parse_config_layer(path: &Path) -> Result<ConfigLayer> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("read config override from {}", path.display()))?;
    let format = match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => ConfigFileFormat::Json,
        _ => ConfigFileFormat::Toml,
    };
    let layer: ConfigLayer = match format {
        ConfigFileFormat::Toml => toml::from_str(&contents)?,
        ConfigFileFormat::Json => serde_json::from_str(&contents)?,
    };
    Ok(layer)
}

fn inferred_project_root_from_config_path(path: &Path) -> Option<PathBuf> {
    path.parent()
        .and_then(|p| {
            p.file_name()
                .and_then(|name| (name == ".greentic").then_some(p))
        })
        .and_then(|config_dir| config_dir.parent())
        .map(|root| root.to_path_buf())
}

fn init_telemetry_from_greentic(config: &HostTelemetryConfig) -> Result<()> {
    if !config.enabled {
        return Ok(());
    }

    let mode = match config.exporter {
        TelemetryExporterKind::Otlp => ExportMode::OtlpGrpc,
        TelemetryExporterKind::Stdout => ExportMode::JsonStdout,
        TelemetryExporterKind::None => return Ok(()),
    };

    let sampling = if (0.0..1.0).contains(&config.sampling) {
        Sampling::TraceIdRatio(config.sampling as f64)
    } else {
        Sampling::AlwaysOn
    };

    let export = ExportConfig {
        mode,
        endpoint: config.endpoint.clone(),
        headers: HashMap::new(),
        sampling,
        compression: None,
    };

    init_telemetry_from_config(
        TelemetryConfig {
            service_name: "greentic-events-cli".to_string(),
        },
        export,
    )
}
