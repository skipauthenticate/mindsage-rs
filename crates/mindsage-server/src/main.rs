//! MindSage — single-binary privacy-first data aggregation server.

use std::path::PathBuf;
use std::sync::Arc;

use tracing::info;
use tracing_subscriber::EnvFilter;

mod indexing;
pub mod migrate;
mod routes;
mod state;

use state::AppState;

fn resolve_data_dir() -> PathBuf {
    std::env::var("MINDSAGE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let exe_dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()));
            if let Some(dir) = exe_dir {
                let parent_data = dir.join("../data");
                if parent_data.exists() {
                    return parent_data;
                }
            }
            PathBuf::from("data")
        })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();

    // Handle CLI subcommands
    if args.len() > 1 {
        match args[1].as_str() {
            "--validate" | "validate" => {
                let data_dir = if args.len() > 2 {
                    PathBuf::from(&args[2])
                } else {
                    resolve_data_dir()
                };
                let report = migrate::validate(&data_dir);
                migrate::print_report(&report);
                std::process::exit(if report.db_valid { 0 } else { 1 });
            }
            "--migrate" | "migrate" => {
                if args.len() < 3 {
                    eprintln!("Usage: mindsage migrate <source-data-dir> [target-data-dir]");
                    std::process::exit(1);
                }
                let source = PathBuf::from(&args[2]);
                let target = if args.len() > 3 {
                    PathBuf::from(&args[3])
                } else {
                    resolve_data_dir()
                };
                let report = migrate::run_migration(&source, &target);
                migrate::print_report(&report);
                std::process::exit(if report.errors.is_empty() { 0 } else { 1 });
            }
            "--help" | "-h" | "help" => {
                println!("MindSage — privacy-first data aggregation server");
                println!();
                println!("Usage: mindsage [command]");
                println!();
                println!("Commands:");
                println!("  (none)                   Start the server");
                println!("  validate [data-dir]      Validate existing database");
                println!("  migrate <src> [dst]      Migrate data from Python installation");
                println!("  help                     Show this help message");
                return Ok(());
            }
            _ => {
                eprintln!("Unknown command: {}. Use 'mindsage help' for usage.", args[1]);
                std::process::exit(1);
            }
        }
    }

    // Normal server startup
    let data_dir = resolve_data_dir();

    info!("Data directory: {}", data_dir.display());

    // Initialize configuration
    let config = mindsage_core::MindSageConfig::from_env(&data_dir)?;
    let port = config.port;

    // Initialize store
    let store = mindsage_store::SqliteStore::open(&config.data_paths.vectordb, config.embedding_dim)
        .map_err(|e| anyhow::anyhow!("Failed to open store: {}", e))?;

    // Initialize embedder (ONNX if available, otherwise BM25-only)
    let model_dir = data_dir.join("models");
    let embedder = mindsage_infer::create_embedder(&model_dir);

    // Build application state
    let state = Arc::new(AppState::new(config, store, embedder));

    // Start background indexing queue
    indexing::start_indexing_worker(state.clone());

    // Build router
    let app = routes::build_router(state.clone());

    // Start server
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("MindSage server listening on {}", addr);

    axum::serve(listener, app).await?;

    Ok(())
}
