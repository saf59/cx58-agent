use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};
use axum::{Router, middleware};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};

use cx58_agent::AppState;
use cx58_agent::handlers::{
    chat_stream_cancel, chat_stream_handler, get_tree_handler, get_user_models_handler,
    health_check, reports_handler, update_report_datetime_handler, update_user_models_handler,
};
use cx58_agent::hmac::{
    RateLimiter, rate_limit_middleware, verify_signature, verify_signature_when_present,
};
use cx58_agent::init::app_init;
use cx58_agent::storage::{MAX_UPLOAD_BODY_BYTES, delete_image_handler, upload_image_handler};
use tower_http::trace::TraceLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

fn create_app_router(state: Arc<AppState>) -> Router {
    let rate_limiter = RateLimiter::new(100, Duration::from_secs(60));

    let protected_routes = Router::new()
        .route("/agent/chat", post(chat_stream_handler))
        .route(
            "/agent/reports/{node_id}",
            post(reports_handler).put(update_report_datetime_handler),
        )
        .route(
            "/agent/models/{user_id}",
            get(get_user_models_handler).put(update_user_models_handler),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            verify_signature,
        ));

    let compatibility_hmac_routes = Router::new()
        .route(
            "/agent/chat/cancel/{request_id}",
            delete(chat_stream_cancel),
        )
        .route("/agent/tree/{user_id}", get(get_tree_handler))
        .route(
            "/agent/images/upload/{parent_id}",
            axum::routing::post(upload_image_handler)
                .layer(DefaultBodyLimit::max(MAX_UPLOAD_BODY_BYTES)),
        )
        //.route("/agent/images", get(get_image_handler))
        .route("/agent/images/{node_id}", delete(delete_image_handler))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            verify_signature_when_present,
        ));

    Router::new()
        .merge(protected_routes)
        .merge(compatibility_hmac_routes)
        // public route
        .route("/agent/health", get(health_check))
        .layer(middleware::from_fn_with_state(
            rate_limiter.clone(),
            rate_limit_middleware,
        ))
        .layer(TraceLayer::new_for_http())
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
    dotenv::dotenv().ok();
    let _ = tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .try_init();

    // Starting AI Agent Server;
    let (config, state) = app_init().await?;
    tracing::info!("Application state initialized");
    let app = create_app_router(state);

    // Start server
    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("Server started!");
    tracing::info!("http://{}", addr);
    tracing::info!("Agent: http://{}/agent/chat", addr);
    tracing::info!("Upload: http://{}/agent/images/upload", addr);
    tracing::info!("Health: http://{}/agent/health", addr);
    tracing::info!("S3: {}", config.s3.bucket);
    tracing::info!("Region: {}", config.s3.region);
    if let Some(ep) = &config.s3.endpoint {
        tracing::info!("Endpoint: {}", ep);
    }
    tracing::info!("CDN: {}", config.s3.public_url_base);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
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
            tracing::info!("Received Ctrl+C, shutting down...");
        },
        _ = terminate => {
            tracing::info!("Received SIGTERM, shutting down...");
        },
    }
}
