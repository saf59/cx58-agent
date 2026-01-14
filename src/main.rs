use axum::Router;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use cx58_agent::handlers::{chat_stream_handler, get_tree_handler, health_check};
use cx58_agent::init::app_init;
use cx58_agent::storage::{delete_image_handler, get_image_handler, upload_image_handler};
use cx58_agent::AppState;

fn create_app_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/agent/chat",
            axum::routing::post(chat_stream_handler),
        )
        .route(
            "/agent/tree/{user_id}",
            axum::routing::get(get_tree_handler),
        )
        .route(
            "/agent/images/upload/{parent_id}",
            axum::routing::post(upload_image_handler),
        )
        .route(
            "/agent/images",
            axum::routing::get(get_image_handler),
        )
        .route(
            "/agent/images",
            axum::routing::delete(delete_image_handler),
        )
        .route("/agent/health", axum::routing::get(health_check))
/*
        .route(
            "/agent/images/batch",
            axum::routing::post(batch_upload_handler),
        )
        .layer(middleware::from_fn(auth_middleware))
*/
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state)
}


#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("Starting AI Agent Server");
    dotenv::dotenv().ok();
    let (config, state) = app_init().await?;
    log::info!("Application state initialized");
    let app = create_app_router(state);

    // Start server
    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    log::info!("Server started!");
    log::info!("http://{}", addr);
    log::info!("Agent: http://{}/agent/chat", addr);
    log::info!("Upload: http://{}/agent/images/upload", addr);
    log::info!("Health: http://{}/agent/health", addr);
    log::info!("S3: {}", config.s3.bucket);
    log::info!("Region: {}", config.s3.region);
    if let Some(ep) = &config.s3.endpoint {
        log::info!("Endpoint: {}", ep);
    }
    log::info!("CDN: {}", config.s3.public_url_base);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            log::info!("Received Ctrl+C, shutting down...");
        },
        _ = terminate => {
            log::info!("Received SIGTERM, shutting down...");
        },
    }
}
