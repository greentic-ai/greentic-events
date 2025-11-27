use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use greentic_events::bus::EventBusBuilder;
use greentic_events::runner::{InMemoryProviderFactory, NoopProvider};

/// Load events providers/bridges from a pack and print descriptors.
#[derive(Parser, Debug)]
#[command(author, version, about = "Greentic Events pack inspector")]
struct Args {
    /// Path to a pack directory containing pack.yaml.
    #[arg(long, value_name = "DIR")]
    pack: PathBuf,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Default to in-memory mocks; caller can swap in Runner* factories when available.
    let provider_factory = InMemoryProviderFactory::new(|| Arc::new(NoopProvider::new()));
    // Bridges are not exercised by this CLI; skip registration.

    let bus = EventBusBuilder::new()
        .register_from_pack_dir(&args.pack, &provider_factory, None)
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
