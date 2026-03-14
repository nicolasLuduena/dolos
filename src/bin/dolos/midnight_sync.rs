//! `dolos midnight-sync` sub-command.
//!
//! Pulls blocks from a Midnight node via Substrate JSON-RPC and persists them
//! to the local archive / state stores.
//!
//! Unlike the Cardano `sync` command this sub-command does **not** require a
//! full `dolos.toml` (no `upstream`, `chain`, or `genesis` fields).
//! Configuration is read from `midnight.toml` (or the file given by `--config`)
//! and any missing fields fall back to sensible defaults.

use dolos_core::config::{
    GenesisConfig, LoggingConfig, RetryConfig, StorageConfig, SyncConfig, TelemetryConfig,
};
use miette::{Context, IntoDiagnostic};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// CLI Args
// ---------------------------------------------------------------------------

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Substrate JSON-RPC HTTP endpoint of the Midnight node.
    ///
    /// Example: `http://localhost:9944`
    #[arg(long, default_value = "http://localhost:9944")]
    pub rpc_url: String,

    /// Path to the midnight-sync TOML config file.
    ///
    /// The file only needs `[storage]`, `[sync]`, `[logging]`, and `[telemetry]`
    /// sections; all sections have sensible defaults.
    #[arg(long)]
    pub config: Option<std::path::PathBuf>,
}

// ---------------------------------------------------------------------------
// Midnight-specific config (no Cardano genesis / upstream required)
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize, Default)]
struct MidnightSyncConfig {
    #[serde(default)]
    storage: StorageConfig,
    #[serde(default)]
    sync: SyncConfig,
    retries: Option<RetryConfig>,
    #[serde(default)]
    logging: LoggingConfig,
    #[serde(default)]
    telemetry: TelemetryConfig,
    /// Cardano genesis files are reused as a placeholder — MidnightLogic
    /// ignores their content entirely.
    #[serde(default)]
    genesis: GenesisConfig,
}

fn load_config(args: &Args) -> MidnightSyncConfig {
    let mut builder = ::config::Config::builder();

    // Look for midnight.toml in /etc/dolos and the working directory.
    builder = builder
        .add_source(::config::File::with_name("/etc/dolos/midnight.toml").required(false))
        .add_source(::config::File::with_name("midnight.toml").required(false));

    if let Some(path) = args.config.as_ref().and_then(|p| p.to_str()) {
        builder = builder.add_source(::config::File::with_name(path).required(true));
    }

    builder = builder
        .add_source(::config::Environment::with_prefix("DOLOS_MIDNIGHT").separator("_"));

    builder
        .build()
        .and_then(|c| c.try_deserialize())
        .unwrap_or_else(|e| {
            tracing::warn!("could not load midnight config, using defaults: {e}");
            MidnightSyncConfig::default()
        })
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
pub async fn run(args: &Args) -> miette::Result<()> {
    // Load the midnight-specific config (no upstream / genesis required).
    let config = load_config(args);

    crate::common::setup_tracing(&config.logging, &config.telemetry)?;

    let domain = setup_midnight_domain(&config)
        .context("setting up midnight domain")?;

    let sync = dolos::sync::midnight_sync(args.rpc_url.clone(), domain, &config.retries)
        .into_diagnostic()
        .context("bootstrapping midnight sync pipeline")?;

    gasket::daemon::Daemon::new(sync).block();

    Ok(())
}

// ---------------------------------------------------------------------------
// Domain setup
// ---------------------------------------------------------------------------

fn setup_midnight_domain(
    config: &MidnightSyncConfig,
) -> miette::Result<dolos::adapters::midnight::MidnightDomainAdapter> {
    use dolos::adapters::{
        midnight::{MidnightDomainAdapter, MidnightWalAdapter},
        ArchiveStoreBackend, IndexStoreBackend, StateStoreBackend,
    };
    use dolos_core::{BootstrapExt, ChainLogic};
    use dolos_midnight::MidnightLogic;
    use miette::IntoDiagnostic;
    use std::sync::Arc;

    /// Create the parent directory of a store path (not the path itself, since
    /// redb expects the path to be a file, not a directory).
    fn ensure_parent(path: &std::path::Path) -> miette::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).into_diagnostic()?;
        }
        Ok(())
    }

    let midnight_schema = dolos_midnight::state_schema();

    // WAL
    let wal_path = config
        .storage
        .wal_path()
        .unwrap_or_else(|| config.storage.path.join("wal"));
    ensure_parent(&wal_path)?;
    let wal: MidnightWalAdapter =
        dolos::adapters::WalStoreBackend::open(&wal_path, &config.storage.wal)
            .into_diagnostic()?;

    // State
    let state_path = config
        .storage
        .state_path()
        .unwrap_or_else(|| config.storage.path.join("state"));
    ensure_parent(&state_path)?;
    let state =
        StateStoreBackend::open(&state_path, midnight_schema.clone(), &config.storage.state)
            .into_diagnostic()?;

    // Archive
    let archive_path = config
        .storage
        .archive_path()
        .unwrap_or_else(|| config.storage.path.join("archive"));
    ensure_parent(&archive_path)?;
    let archive =
        ArchiveStoreBackend::open(&archive_path, midnight_schema, &config.storage.archive)
            .into_diagnostic()?;

    // Indexes
    let index_path = config
        .storage
        .index_path()
        .unwrap_or_else(|| config.storage.path.join("index"));
    ensure_parent(&index_path)?;
    let indexes = IndexStoreBackend::open(&index_path, &config.storage.index).into_diagnostic()?;

    // Mempool: ephemeral in-memory store (no persistence needed yet).
    let mempool = dolos::adapters::MempoolBackend::Ephemeral(
        dolos_core::builtin::EphemeralMempool::new(),
    );

    // MidnightLogic::initialize ignores genesis entirely.  We load the
    // Cardano genesis files purely to satisfy the type signature; their
    // content is never read by Midnight chain logic.
    let genesis = Arc::new(
        crate::common::open_genesis_files(&config.genesis)?,
    );

    let chain = MidnightLogic::initialize::<MidnightDomainAdapter>(
        Default::default(),
        &state,
        &genesis,
    )
    .into_diagnostic()?;

    let (tip_broadcast, _) = tokio::sync::broadcast::channel(100);

    let domain = MidnightDomainAdapter {
        storage_config: Arc::new(config.storage.clone()),
        sync_config: Arc::new(config.sync.clone()),
        genesis,
        chain: Arc::new(std::sync::RwLock::new(chain)),
        wal,
        state,
        archive,
        indexes,
        mempool,
        tip_broadcast,
    };

    domain
        .bootstrap()
        .map_err(|e| miette::miette!("{:?}", e))?;

    Ok(domain)
}
