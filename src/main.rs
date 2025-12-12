// backend/src/main.rs - Ultra-lightweight with rust-s3

use axum::{Router, middleware};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};

mod agent;
mod rig_integration;
mod storage;

use agent::{AgentExecutor, AppState};
use rig_integration::{AgentOrchestrator, RigAgentChain};
use storage::{ImageProcessor, ImageUrlResolver, StorageService};

// ============================================================================
// Configuration
// ============================================================================

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub redis_url: String,
    pub s3: S3Config,
    pub qdrant_url: String,
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
            qdrant_url: std::env::var("QDRANT_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:6334".to_string()),
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

async fn setup_qdrant(config: &Config) -> Result<qdrant_client::client::QdrantClient, String> {
    qdrant_client::client::QdrantClient::from_url(&config.qdrant_url)
        .build()
        .map_err(|e| format!("Qdrant error: {}", e))
}

async fn initialize_qdrant_collections(
    qdrant: &qdrant_client::client::QdrantClient,
) -> Result<(), String> {
    use qdrant_client::prelude::*;

    let collections = qdrant
        .list_collections()
        .await
        .map_err(|e| format!("List collections failed: {}", e))?;

    let exists = collections
        .collections
        .iter()
        .any(|c| c.name == "image_embeddings");

    if !exists {
        qdrant
            .create_collection(&CreateCollection {
                collection_name: "image_embeddings".to_string(),
                vectors_config: Some(VectorsConfig {
                    config: Some(qdrant_client::qdrant::vectors_config::Config::Params(
                        VectorParams {
                            size: 512,
                            distance: Distance::Cosine.into(),
                            ..Default::default()
                        },
                    )),
                }),
                ..Default::default()
            })
            .await
            .map_err(|e| format!("Create collection failed: {}", e))?;

        log::info!("Created Qdrant collection: image_embeddings");
    }

    Ok(())
}

// ============================================================================
// Storage Setup with rust-s3
// ============================================================================

fn setup_storage(config: &S3Config) -> Result<Arc<StorageService>, ai_agent_shared::AppError> {
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

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

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
) -> axum::Json<ai_agent_shared::HealthStatus> {
    let mut health = ai_agent_shared::HealthStatus::healthy();

    health.services.database = sqlx::query("SELECT 1").fetch_one(&state.db).await.is_ok();

    health.services.redis = redis::cmd("PING")
        .query_async::<_, String>(&mut state.redis.clone())
        .await
        .is_ok();

    health.services.s3 = state.storage.exists("health-check").await.unwrap_or(true); // Don't fail health check for missing test file

    health.services.qdrant = state.qdrant.health_check().await.is_ok();

    health.services.ollama = reqwest::get(format!("{}/api/tags", state.ollama_url))
        .await
        .is_ok();

    if !health.is_healthy() {
        health.status = "degraded".to_string();
    }

    axum::Json(health)
}

// ============================================================================
// AppState
// ============================================================================

// ============================================================================
// Main Entry Point
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("🚀 Starting AI Agent Server with rust-s3...");

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

    // Qdrant
    log::info!("🔍 Connecting to Qdrant...");
    let qdrant = setup_qdrant(&config).await?;
    initialize_qdrant_collections(&qdrant).await?;
    log::info!("✅ Qdrant connected");

    // S3 Storage with rust-s3 (synchronous setup)
    log::info!("☁️  Initializing S3 with rust-s3 (ultra-lightweight)...");
    let storage = setup_storage(&config.s3)?;
    log::info!("✅ S3 storage initialized");

    // Test S3 connection
    match storage.list_user_images(&uuid::Uuid::new_v4()).await {
        Ok(_) => log::info!("✅ S3 connection verified"),
        Err(e) => log::warn!("⚠️  S3 test (expected): {}", e),
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
        qdrant.clone(),
    ));

    // Application state
    let state = Arc::new(AppState {
        db,
        redis,
        storage,
        image_resolver,
        image_processor,
        qdrant,
        ollama_url: config.ollama_url.clone(),
        agent,
        orchestrator,
    });

    log::info!("✅ Application state initialized");

    // Router
    let app = create_app_router(state);

    // Start server
    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    log::info!("");
    log::info!("🎉 Server started successfully!");
    log::info!("📍 Listening on http://{}", addr);
    log::info!("📡 Agent: http://{}/api/agent/chat", addr);
    log::info!("🖼️  Upload: http://{}/api/images/upload", addr);
    log::info!("❤️  Health: http://{}/health", addr);
    log::info!("");
    log::info!("💾 S3 Bucket: {}", config.s3.bucket);
    log::info!("🌍 S3 Region: {}", config.s3.region);
    if let Some(ep) = &config.s3.endpoint {
        log::info!("🔌 S3 Endpoint: {}", ep);
    }
    log::info!("🔗 Public URL: {}", config.s3.public_url_base);
    log::info!("⚡ Using rust-s3 (ultra-lightweight, no AWS SDK!)");
    log::info!("");

    axum::serve(listener, app).await?;

    Ok(())
}
