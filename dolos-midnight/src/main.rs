use std::sync::{Arc, RwLock};

use clap::{Parser, Subcommand};
use tracing::info;

use dolos_core::config::{StorageConfig, SyncConfig};
use dolos_core::{ChainLogic, TipEvent};

mod api;
mod block_source;
mod chain;
mod config;
mod decrypt;
mod domain;
mod model;
mod storage_init;
mod sync_loop;
mod tx_parser;
mod work;

use chain::{MidnightChainConfig, MidnightGenesis, MidnightLogic};
use config::MidnightConfig;
use decrypt::MockDecryptor;
use domain::{MidnightDomain, StubMempool};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "dolos-midnight")]
#[command(about = "Midnight blockchain data node (PoC)")]
struct Cli {
    /// Path to config file
    #[arg(long, default_value = "midnight.toml")]
    config: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Write a default config file and create storage directories
    Init,
    /// Run the sync loop and REST API server
    Daemon,
}

// ---------------------------------------------------------------------------
// Init command
// ---------------------------------------------------------------------------

fn run_init(config_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = MidnightConfig::default();
    let toml_str = toml::to_string_pretty(&config)?;

    if std::path::Path::new(config_path).exists() {
        eprintln!("Config file already exists: {config_path}");
        return Ok(());
    }

    std::fs::write(config_path, &toml_str)?;
    info!(path = config_path, "wrote default config");

    std::fs::create_dir_all(&config.storage.path)?;
    info!(path = ?config.storage.path, "created storage directory");

    println!("Initialized dolos-midnight at {config_path}");
    Ok(())
}

// ---------------------------------------------------------------------------
// Daemon command
// ---------------------------------------------------------------------------

async fn run_daemon(config_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config_contents = std::fs::read_to_string(config_path)?;
    let config: MidnightConfig = toml::from_str(&config_contents)?;

    info!(ws_url = %config.node.ws_url, "starting daemon");

    // Open storage
    let (wal, state, archive, indexes) = storage_init::open_stores(&config.storage.path)?;

    // Initialize chain logic
    let genesis = MidnightGenesis {
        ws_url: config.node.ws_url.clone(),
        network_name: "midnight".to_string(),
    };

    let chain = MidnightLogic::initialize::<MidnightDomain>(
        MidnightChainConfig,
        &state,
        genesis.clone(),
    )?;

    let (tip_broadcast, _) = tokio::sync::broadcast::channel::<TipEvent>(100);

    let storage_config = StorageConfig {
        path: config.storage.path.clone(),
        ..Default::default()
    };

    let domain = MidnightDomain {
        wal,
        chain: Arc::new(RwLock::new(chain)),
        state,
        archive,
        indexes,
        mempool: StubMempool,
        storage_config,
        sync_config: SyncConfig::default(),
        genesis: Arc::new(genesis),
        tip_broadcast,
    };

    // Build API router
    let decryptor = MockDecryptor;
    let app = api::router(domain.clone(), decryptor);

    // Spawn sync loop + API server concurrently
    let sync_domain = domain.clone();
    let source = block_source::MockBlockSource;

    let sync_handle = tokio::spawn(async move {
        if let Err(e) = sync_loop::run_sync(sync_domain, source).await {
            tracing::error!(error = %e, "sync loop failed");
        }
    });

    let listen_addr = config.api.listen_address;
    info!(%listen_addr, "starting REST API server");

    let listener = tokio::net::TcpListener::bind(listen_addr).await?;
    let api_handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!(error = %e, "API server failed");
        }
    });

    // Wait for either task to finish (or Ctrl+C)
    tokio::select! {
        _ = sync_handle => info!("sync loop stopped"),
        _ = api_handle => info!("API server stopped"),
        _ = tokio::signal::ctrl_c() => info!("received shutdown signal"),
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init => run_init(&cli.config),
        Commands::Daemon => run_daemon(&cli.config).await,
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
