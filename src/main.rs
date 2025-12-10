// main.rs - Backend Entry Point

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
use storage::{ImageProcessor, ImageUrlResolver, S3Config, StorageService};

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
// Application Setup
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
        .map_err(|e| format!("Qdrant connection failed: {}", e))
}

async fn initialize_qdrant_collections(
    qdrant: &qdrant_client::client::QdrantClient,
) -> Result<(), String> {
    use qdrant_client::prelude::*;

    // Create image_embeddings collection if not exists
    let collections = qdrant
        .list_collections()
        .await
        .map_err(|e| format!("Failed to list collections: {}", e))?;

    let collection_exists = collections
        .collections
        .iter()
        .any(|c| c.name == "image_embeddings");

    if !collection_exists {
        qdrant
            .create_collection(&CreateCollection {
                collection_name: "image_embeddings".to_string(),
                vectors_config: Some(VectorsConfig {
                    config: Some(qdrant_client::qdrant::vectors_config::Config::Params(
                        VectorParams {
                            size: 512, // Adjust based on your embedding model
                            distance: Distance::Cosine.into(),
                            ..Default::default()
                        },
                    )),
                }),
                ..Default::default()
            })
            .await
            .map_err(|e| format!("Failed to create collection: {}", e))?;

        log::info!("Created Qdrant collection: image_embeddings");
    }

    Ok(())
}

// ============================================================================
// Middleware
// ============================================================================

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

/// Extract user context from JWT (implement based on your OIDC setup)
async fn auth_middleware(mut request: Request, next: Next) -> Result<Response, StatusCode> {
    // Your OIDC JWT validation here
    // For now, we'll extract from headers

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

    // Store in request extensions
    request.extensions_mut().insert(user_id);
    request.extensions_mut().insert(session_id);
    request.extensions_mut().insert(language);
    request.extensions_mut().insert(chat_id);

    Ok(next.run(request).await)
}

// ============================================================================
// Router Setup
// ============================================================================

fn create_app_router(state: Arc<AppState>) -> Router {
    Router::new()
        // Agent routes
        .route(
            "/api/agent/chat",
            axum::routing::post(agent::chat_stream_handler_with_images),
        )
        .route(
            "/api/agent/tree/:user_id/:root_id",
            axum::routing::get(agent::get_tree_handler),
        )
        // Image management routes
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
        .route(
            "/api/images/:node_id/thumbnail",
            axum::routing::post(agent::create_thumbnail_handler),
        )
        .route(
            "/api/images/:node_id/download",
            axum::routing::get(agent::get_download_url_handler),
        )
        // Image import routes
        .route(
            "/api/images/import",
            axum::routing::post(agent::import_image_handler),
        )
        .route(
            "/api/images/import/batch",
            axum::routing::post(agent::batch_import_handler),
        )
        // Admin routes
        .route(
            "/api/admin/migrate/:user_id",
            axum::routing::post(agent::migrate_user_images_handler),
        )
        // Health check
        .route("/health", axum::routing::get(health_check))
        // Apply auth middleware to all routes
        .layer(middleware::from_fn(auth_middleware))
        // CORS
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state)
}

async fn health_check() -> &'static str {
    "OK"
}

// ============================================================================
// Main Entry Point
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    log::info!("Starting AI Agent Server...");

    // Load configuration
    dotenv::dotenv().ok();
    let config = Config::from_env()?;
    log::info!("Configuration loaded");

    // Setup database
    log::info!("Connecting to PostgreSQL...");
    let db = setup_database(&config).await?;
    log::info!("PostgreSQL connected");

    // Run migrations
    log::info!("Running database migrations...");
    sqlx::migrate!("./migrations").run(&db).await?;
    log::info!("Migrations completed");

    // Setup Redis
    log::info!("Connecting to Redis...");
    let redis = setup_redis(&config).await?;
    log::info!("Redis connected");

    // Setup Qdrant
    log::info!("Connecting to Qdrant...");
    let qdrant = setup_qdrant(&config).await?;
    initialize_qdrant_collections(&qdrant).await?;
    log::info!("Qdrant connected and initialized");

    // Setup S3 Storage
    log::info!("Initializing S3 storage...");
    let storage = Arc::new(StorageService::new(config.s3.clone())?);
    log::info!("S3 storage initialized");

    // Create resolvers and processors
    let image_resolver = Arc::new(ImageUrlResolver {
        storage: storage.clone(),
        db: db.clone(),
    });

    let image_processor = Arc::new(ImageProcessor::new(storage.clone()));

    // Create agent
    let agent = Arc::new(RwLock::new(AgentExecutor::new(config.ollama_url.clone())));

    // Create orchestrator
    let orchestrator = Arc::new(AgentOrchestrator::new(
        &config.ollama_url,
        db.clone(),
        qdrant.clone(),
    ));

    // Create application state
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

    log::info!("Application state initialized");

    // Create router
    let app = create_app_router(state);

    // Start server
    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    log::info!("🚀 Server listening on http://{}", addr);
    log::info!("📡 Agent endpoint: http://{}/api/agent/chat", addr);
    log::info!("🖼️  Image upload: http://{}/api/images/upload", addr);

    axum::serve(listener, app).await?;

    Ok(())
}

// ============================================================================
// Updated AppState with Orchestrator
// ============================================================================

pub struct AppState {
    pub db: sqlx::PgPool,
    pub redis: redis::aio::ConnectionManager,
    pub storage: Arc<StorageService>,
    pub image_resolver: Arc<ImageUrlResolver>,
    pub image_processor: Arc<ImageProcessor>,
    pub qdrant: qdrant_client::client::QdrantClient,
    pub ollama_url: String,
    pub agent: Arc<RwLock<AgentExecutor>>,
    pub orchestrator: Arc<AgentOrchestrator>,
}
