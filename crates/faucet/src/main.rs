//! Faucet service binary

use clap::Parser;
use norn_faucet::api::{dispense_handler, health_handler, root_handler, status_handler};
use norn_faucet::{FaucetConfig, FaucetService};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Faucet service CLI
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Config file path
    #[arg(short, long)]
    config: Option<String>,

    /// Server address
    #[arg(long)]
    server_addr: Option<String>,

    /// RPC URL
    #[arg(long)]
    rpc_url: Option<String>,

    /// Private key
    #[arg(long)]
    private_key: Option<String>,

    /// Dispense amount (in wei)
    #[arg(long)]
    dispense_amount: Option<String>,

    /// Rate limit window (seconds)
    #[arg(long)]
    rate_limit_window: Option<u64>,

    /// Enable debug logging
    #[arg(long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize tracing
    let env_filter = if args.debug {
        tracing_subscriber::EnvFilter::new("debug")
    } else {
        tracing_subscriber::EnvFilter::from_default_env()
    };

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting Norn Faucet Service v0.1.0");

    // Load configuration
    let mut config = if let Some(config_path) = args.config {
        // Load from file (would need to implement file loading)
        FaucetConfig::from_env()
    } else {
        FaucetConfig::from_env()
    };

    // Override with CLI arguments
    if let Some(addr) = args.server_addr {
        config.server_addr = addr;
    }

    if let Some(rpc_url) = args.rpc_url {
        config.rpc_url = rpc_url;
    }

    if let Some(key) = args.private_key {
        config.private_key = key;
    }

    if let Some(amount) = args.dispense_amount {
        config.dispense_amount = amount;
    }

    if let Some(window) = args.rate_limit_window {
        config.rate_limit_window_secs = window;
    }

    info!("Configuration:");
    info!("  Server address: {}", config.server_addr);
    info!("  RPC URL: {}", config.rpc_url);
    info!("  Dispense amount: {} wei", config.dispense_amount);
    info!("  Rate limit: {} requests / {}s", config.max_requests_per_window, config.rate_limit_window_secs);
    info!("  Address cooldown: {}s", config.address_cooldown_secs);

    // Initialize database
    let database = norn_faucet::FaucetDatabase::new(&config.db_path)?;
    info!("Database initialized at: {}", config.db_path);

    // Print statistics
    let stats = database.get_statistics()?;
    info!("Previous statistics:");
    info!("  Total distributions: {}", stats.total_distributions);
    info!("  Unique addresses: {}", stats.unique_addresses);

    // Create faucet service
    let service = Arc::new(FaucetService::new(config.clone(), database)?);
    info!("Faucet service initialized");

    // Build router
    let mut app = axum::Router::new()
        .route("/", axum::routing::get(root_handler))
        .route("/health", axum::routing::get(health_handler))
        .route("/api/status", axum::routing::get(status_handler))
        .route("/api/dispense", axum::routing::post(dispense_handler))
        .with_state(service.clone());

    // Add CORS if enabled
    if config.cors_enabled {
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        app = app.layer(cors);
        info!("CORS enabled");
    }

    // Start cleanup task
    let database_clone = service.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(86400)); // Daily cleanup
        loop {
            interval.tick().await;
            match database_clone.cleanup_old_records(30) {
                Ok(count) => info!("Cleaned up {} old records", count),
                Err(e) => warn!("Cleanup failed: {:?}", e),
            }
        }
    });

    // Start server
    let addr: SocketAddr = config.server_addr.parse()?;
    info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("Shutting down gracefully");
    Ok(())
}

/// Graceful shutdown signal
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C");
        },
        _ = terminate => {
            info!("Received terminate signal");
        },
    }
}
