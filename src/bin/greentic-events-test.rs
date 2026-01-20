use std::path::PathBuf;

use anyhow::{Result, anyhow};
use clap::{ArgAction, Parser, Subcommand};
use greentic_events::e2e::{E2eOptions, run_e2e};
use greentic_telemetry::export::{ExportConfig, ExportMode, Sampling};
use greentic_telemetry::init::{TelemetryConfig, init_telemetry_from_config};

const LIVE_ENV_FLAG: &str = "GREENTIC_EVENTS_E2E_LIVE";
const NETWORK_ENV_FLAG: &str = "GREENTIC_EVENTS_E2E_ALLOW_NETWORK";

#[derive(Parser, Debug)]
#[command(author, version, about = "Greentic Events test CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run end-to-end conformance checks against events packs.
    E2e {
        /// Directory containing events-*.gtpack files.
        #[arg(long, value_name = "DIR")]
        packs: PathBuf,

        /// Filter to a single provider name.
        #[arg(long)]
        provider: Option<String>,

        /// Optional JSON report output path.
        #[arg(long, value_name = "FILE")]
        report: Option<PathBuf>,

        /// Run in dry-run mode (default).
        #[arg(long, action = ArgAction::SetTrue)]
        dry_run: bool,

        /// Run against live providers (requires env gating).
        #[arg(long, action = ArgAction::SetTrue)]
        live: bool,

        /// Enable trace output via Greentic telemetry.
        #[arg(long, action = ArgAction::SetTrue)]
        trace: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::E2e {
            packs,
            provider,
            report,
            dry_run: _dry_run,
            live,
            trace,
        } => {
            if live && _dry_run {
                return Err(anyhow!("--live conflicts with --dry-run"));
            }
            let dry_run = !live;
            if live {
                ensure_live_allowed()?;
            }
            if trace {
                init_trace()?;
            }
            let options = E2eOptions {
                packs_dir: packs,
                provider_filter: provider,
                report_path: report,
                dry_run,
                live,
            };
            let report = run_e2e(options).await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
    }
}

fn ensure_live_allowed() -> Result<()> {
    let live = std::env::var(LIVE_ENV_FLAG).unwrap_or_default() == "1";
    let network = std::env::var(NETWORK_ENV_FLAG).unwrap_or_default() == "1";
    if live && network {
        Ok(())
    } else {
        Err(anyhow!(
            "--live requires {}=1 and {}=1",
            LIVE_ENV_FLAG,
            NETWORK_ENV_FLAG
        ))
    }
}

fn init_trace() -> Result<()> {
    let export = ExportConfig {
        mode: ExportMode::JsonStdout,
        endpoint: None,
        headers: Default::default(),
        sampling: Sampling::AlwaysOn,
        compression: None,
    };
    init_telemetry_from_config(
        TelemetryConfig {
            service_name: "greentic-events-test".into(),
        },
        export,
    )
}
