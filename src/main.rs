use axum::{middleware, Router};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

use cx58_agent::handlers::{auth_middleware, chat_stream_handler, get_tree_handler, health_check};
use cx58_agent::init::app_init;
use cx58_agent::storage::{batch_upload_handler, delete_image_handler, get_image_handler, upload_image_handler};
use cx58_agent::AppState;

fn create_app_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/api/agent/chat",
            axum::routing::post(chat_stream_handler),
        )
        .route(
            "/api/agent/tree/:user_id/:root_id",
            axum::routing::get(get_tree_handler),
        )
        .route(
            "/api/images/upload",
            axum::routing::post(upload_image_handler),
        )
        .route(
            "/api/images/:node_id",
            axum::routing::get(get_image_handler),
        )
        .route(
            "/api/images/:node_id",
            axum::routing::delete(delete_image_handler),
        )
        .route(
            "/api/images/batch",
            axum::routing::post(batch_upload_handler),
        )
        .route("/health", axum::routing::get(health_check))
        .layer(middleware::from_fn(auth_middleware))
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
    log::info!("🚀 Starting AI Agent Server (lightweight, no embeddings)...");
    dotenv::dotenv().ok();
    let (config, state) = app_init().await?;
    log::info!("✅ Application state initialized");
    let app = create_app_router(state);

    // Start server
    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    log::info!("");
    log::info!("🎉 Server started!");
    log::info!("📍 http://{}", addr);
    log::info!("📡 Agent: http://{}/api/agent/chat", addr);
    log::info!("🖼️  Upload: http://{}/api/images/upload", addr);
    log::info!("❤️  Health: http://{}/health", addr);
    log::info!("");
    log::info!("💾 S3: {}", config.s3.bucket);
    log::info!("🌍 Region: {}", config.s3.region);
    if let Some(ep) = &config.s3.endpoint {
        log::info!("🔌 Endpoint: {}", ep);
    }
    log::info!("🔗 CDN: {}", config.s3.public_url_base);
    log::info!("⚡ rust-s3 + Ollama (NO embeddings, NO Qdrant)");
    log::info!("");

    axum::serve(listener, app).await?;

    Ok(())
}

