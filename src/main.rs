// backend/src/main.rs - No embeddings/Qdrant

use axum::{Router, middleware};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

mod agent;
mod storage;
use agent::{AgentExecutor, AppState};
use storage::{ImageProcessor, ImageUrlResolver, StorageService};

// ============================================================================
// Configuration
// ============================================================================

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub redis_url: String,
    pub s3: S3Config,
    pub ollama_url: String,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>,
    pub access_key: String,
    pub secret_key: String,
    pub public_url_base: String,
}

impl Config {
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            database_url: std::env::var("DATABASE_URL")?,
            redis_url: std::env::var("REDIS_URL")
                .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string()),
            s3: S3Config {
                bucket: std::env::var("S3_BUCKET")?,
                region: std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
                endpoint: std::env::var("S3_ENDPOINT").ok(),
                access_key: std::env::var("AWS_ACCESS_KEY_ID")?,
                secret_key: std::env::var("AWS_SECRET_ACCESS_KEY")?,
                public_url_base: std::env::var("S3_PUBLIC_URL")?,
            },
            ollama_url: std::env::var("OLLAMA_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:11434".to_string()),
            host: std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: std::env::var("PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()?,
        })
    }
}

// ============================================================================
// Setup Functions
// ============================================================================

async fn setup_database(config: &Config) -> Result<sqlx::PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(50)
        .connect(&config.database_url)
        .await
}

async fn setup_redis(config: &Config) -> Result<redis::aio::ConnectionManager, redis::RedisError> {
    let client = redis::Client::open(config.redis_url.as_str())?;
    redis::aio::ConnectionManager::new(client).await
}

fn setup_storage(config: &S3Config) -> Result<Arc<StorageService>, AppError> {
    let storage = StorageService::new(
        config.bucket.clone(),
        config.region.clone(),
        config.access_key.clone(),
        config.secret_key.clone(),
        config.public_url_base.clone(),
        config.endpoint.clone(),
    )?;

    Ok(Arc::new(storage))
}

// ============================================================================
// Middleware
// ============================================================================

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use cx58_agent::error::AppError;
use cx58_agent::models::HealthStatus;
use cx58_agent::rig_integration::AgentOrchestrator;

async fn auth_middleware(mut request: Request, next: Next) -> Result<Response, StatusCode> {
    let user_id = request
        .headers()
        .get("X-User-ID")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let session_id = request
        .headers()
        .get("X-Session-ID")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let language = request
        .headers()
        .get("X-Language")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "en".to_string());

    let chat_id = request
        .headers()
        .get("X-Chat-ID")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .unwrap_or_else(uuid::Uuid::new_v4);

    request.extensions_mut().insert(user_id);
    request.extensions_mut().insert(session_id);
    request.extensions_mut().insert(language);
    request.extensions_mut().insert(chat_id);

    Ok(next.run(request).await)
}

// ============================================================================
// Router
// ============================================================================

fn create_app_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/api/agent/chat",
            axum::routing::post(agent::chat_stream_handler_with_images),
        )
        .route(
            "/api/agent/tree/:user_id/:root_id",
            axum::routing::get(agent::get_tree_handler),
        )
        .route(
            "/api/images/upload",
            axum::routing::post(storage::upload_image_handler),
        )
        .route(
            "/api/images/:node_id",
            axum::routing::get(storage::get_image_handler),
        )
        .route(
            "/api/images/:node_id",
            axum::routing::delete(storage::delete_image_handler),
        )
        .route(
            "/api/images/batch",
            axum::routing::post(storage::batch_upload_handler),
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

async fn health_check(
    State(state): State<Arc<AppState>>,
) -> axum::Json<HealthStatus> {
    let mut health = HealthStatus::healthy();

    health.services.database = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();

    health.services.redis = redis::cmd("PING")
        .query_async::<_, String>(&mut state.redis.clone())
        .await
        .is_ok();

    health.services.s3 = state.storage.exists("health-check").await.unwrap_or(true);

    health.services.ollama = reqwest::get(format!("{}/api/tags", state.ollama_url))
        .await
        .is_ok();

    if !health.is_healthy() {
        health.status = "degraded".to_string();
    }

    axum::Json(health)
}

// ============================================================================
// Main Entry Point
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("🚀 Starting AI Agent Server (lightweight, no embeddings)...");

    dotenv::dotenv().ok();
    let config = Config::from_env()?;
    log::info!("✅ Configuration loaded");

    // Database
    log::info!("📊 Connecting to PostgreSQL...");
    let db = setup_database(&config).await?;
    log::info!("✅ PostgreSQL connected");

    log::info!("🔄 Running migrations...");
    sqlx::migrate!("./migrations").run(&db).await?;
    log::info!("✅ Migrations completed");

    // Redis
    log::info!("📮 Connecting to Redis...");
    let redis = setup_redis(&config).await?;
    log::info!("✅ Redis connected");

    // S3 Storage
    log::info!("☁️  Initializing S3 with rust-s3...");
    let storage = setup_storage(&config.s3)?;
    log::info!("✅ S3 storage initialized");

    // Test S3
    match storage.list_user_images(&uuid::Uuid::new_v4()).await {
        Ok(_) => log::info!("✅ S3 connection verified"),
        Err(e) => log::warn!("⚠️  S3 test: {}", e),
    }

    // Resolvers and processors
    let image_resolver = Arc::new(ImageUrlResolver {
        storage: storage.clone(),
        db: db.clone(),
    });

    let image_processor = Arc::new(ImageProcessor::new(storage.clone()));

    // Agent
    let agent = Arc::new(RwLock::new(AgentExecutor::new(config.ollama_url.clone())));
    // Orchestrator
    let orchestrator = Arc::new(AgentOrchestrator::new(
        &config.ollama_url,
        db.clone(),
    ));

    // Application state
    let state = Arc::new(AppState {
        db,
        redis,
        storage,
        image_resolver,
        image_processor,
        ollama_url: config.ollama_url.clone(),
        agent,
        orchestrator
    });

    log::info!("✅ Application state initialized");

    // Router
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
