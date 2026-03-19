use clap::{Args, Subcommand};
use miette::{IntoDiagnostic, Result};
use std::path::Path;
use dolos_midnight::{MidnightConfig, start_sync, MidnightDomain, MidnightLogic, MidnightGenesis, MidnightDelta};
use dolos_core::config::{StorageConfig, SyncConfig};
use std::sync::{Arc, RwLock};
use subxt::{OnlineClient, SubstrateConfig};

#[derive(Args, Debug)]
pub struct MidnightArgs {
    #[command(subcommand)]
    pub command: MidnightCommand,
}

#[derive(Subcommand, Debug)]
pub enum MidnightCommand {
    /// Initialize the midnight node configuration
    Init(InitArgs),
    /// Run the midnight indexer daemon
    Daemon(DaemonArgs),
}

#[derive(Args, Debug)]
pub struct InitArgs {
    #[arg(short, long, default_value = "dolos-midnight.toml")]
    pub config: std::path::PathBuf,
}

#[derive(Args, Debug)]
pub struct DaemonArgs {
    #[arg(short, long, default_value = "dolos-midnight.toml")]
    pub config: std::path::PathBuf,
}

pub fn run(args: &MidnightArgs) -> Result<()> {
    match &args.command {
        MidnightCommand::Init(args) => run_init(args),
        MidnightCommand::Daemon(args) => run_daemon(args),
    }
}

fn run_init(args: &InitArgs) -> Result<()> {
    if args.config.exists() {
        println!("Configuration file already exists: {:?}", args.config);
        return Ok(());
    }

    let default_config = MidnightConfig::default();
    let toml_string = toml::to_string_pretty(&default_config).into_diagnostic()?;
    std::fs::write(&args.config, toml_string).into_diagnostic()?;

    println!("Initialized Midnight configuration in {:?}", args.config);
    Ok(())
}

fn run_daemon(args: &DaemonArgs) -> Result<()> {
    crate::common::setup_tracing_error_only()?;

    let config: MidnightConfig = if args.config.exists() {
        let content = std::fs::read_to_string(&args.config).into_diagnostic()?;
        toml::from_str(&content).into_diagnostic()?
    } else {
        println!("Configuration not found, using defaults");
        MidnightConfig::default()
    };

    println!("Starting Midnight indexer with RPC: {}", config.rpc_url);

    let rt = tokio::runtime::Runtime::new().into_diagnostic()?;
    
    rt.block_on(async {
        // Fetch metadata first
        println!("Fetching chain metadata...");
        let client = OnlineClient::<SubstrateConfig>::from_url(&config.rpc_url).await.into_diagnostic()?;
        let genesis_hash = client.genesis_hash().0;

        // Setup Dolos storage
        let storage_path = Path::new(&config.data_dir);
        std::fs::create_dir_all(storage_path).into_diagnostic()?;

        let storage_config = StorageConfig {
            path: storage_path.to_path_buf(),
            ..Default::default()
        };

        let state_store = dolos_fjall::StateStore::open(&storage_path.join("state"), &Default::default()).into_diagnostic()?;
        let archive_store = dolos_redb3::archive::ArchiveStore::open(Default::default(), &storage_path.join("archive"), &Default::default()).into_diagnostic()?;
        let index_store = dolos_fjall::IndexStore::open(&storage_path.join("index"), &Default::default()).into_diagnostic()?;
        let wal_store = dolos_redb3::wal::RedbWalStore::<MidnightDelta>::open(&storage_path.join("wal"), &Default::default()).into_diagnostic()?;
        let mempool = dolos_core::builtin::EphemeralMempool::new();
        
        let genesis = Arc::new(MidnightGenesis { chain_id: 0, genesis_hash });
        let logic = MidnightLogic { queue: std::collections::VecDeque::new() };
        
        let domain = MidnightDomain {
            storage_config: Arc::new(storage_config),
            sync_config: Arc::new(SyncConfig::default()),
            state: state_store,
            archive: archive_store,
            indexes: index_store,
            mempool,
            wal: wal_store,
            chain: Arc::new(RwLock::new(logic)),
            genesis,
            subxt: Arc::new(client),
        };

        let api_domain = domain.clone();
        tokio::spawn(async move {
            dolos_midnight::api::serve_api(api_domain, 8080).await;
        });

        start_sync(config, domain).await.map_err(|e| miette::miette!(e.to_string()))
    })?;

    Ok(())
}
