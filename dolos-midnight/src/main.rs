// Phase 0 stub crate — many types are defined ahead of use.
#![allow(dead_code)]

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
#[cfg(not(feature = "midnight-crypto"))]
use decrypt::MockDecryptor;
#[cfg(feature = "midnight-crypto")]
use decrypt::RealDecryptor;
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

    info!(
        ws_url = %config.node.ws_url,
        storage_path = ?config.storage.path,
        api_listen = %config.api.listen_address,
        "starting daemon"
    );

    // Open storage
    info!(path = ?config.storage.path, "opening storage");
    let (wal, state, archive, indexes) = storage_init::open_stores(&config.storage.path)?;
    info!("storage opened successfully");

    // Initialize chain logic
    let genesis = MidnightGenesis {
        ws_url: config.node.ws_url.clone(),
        network_name: "midnight".to_string(),
    };

    info!("initializing chain logic");
    #[allow(unused_mut)]
    let mut chain = MidnightLogic::initialize::<MidnightDomain>(
        MidnightChainConfig,
        &state,
        genesis.clone(),
    )?;

    // Inject real parser when subxt-sync feature is enabled
    #[cfg(feature = "subxt-sync")]
    {
        info!("using SubxtParser (real midnight-ledger tx parsing)");
        chain.set_parser(std::sync::Arc::new(tx_parser::SubxtParser));
    }
    #[cfg(not(feature = "subxt-sync"))]
    info!("using MockParser (no real tx parsing — enable subxt-sync feature)");

    #[cfg(feature = "midnight-crypto")]
    info!("using RealDecryptor (midnight-crypto enabled)");
    #[cfg(not(feature = "midnight-crypto"))]
    info!("using MockDecryptor (trial decryption disabled — enable midnight-crypto feature)");

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
    #[cfg(not(feature = "midnight-crypto"))]
    let decryptor = MockDecryptor;
    #[cfg(feature = "midnight-crypto")]
    let decryptor = RealDecryptor;
    let app = api::router(domain.clone(), decryptor);

    // Spawn sync loop + API server concurrently
    let sync_domain = domain.clone();

    let sync_handle = tokio::spawn(async move {
        #[cfg(feature = "subxt-sync")]
        let source = block_source::SubxtBlockSource::new(
            config.node.ws_url.clone(),
            config.node.sync_batch_size,
        );

        #[cfg(not(feature = "subxt-sync"))]
        let source = block_source::MockBlockSource;

        if let Err(e) = sync_loop::run_sync(sync_domain, source, config.node.sync_batch_size).await {
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
