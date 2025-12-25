use std::error::Error;
use std::sync::Arc;
use sqlx::postgres::PgPoolOptions;
use crate::{AiConfig, AppState, MasterAgent};
use crate::error::AppError;
use crate::handlers::{ImageProcessor, ImageUrlResolver, StorageService};

// ============================================================================
// Configuration
// ============================================================================

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub s3: S3Config,
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
            s3: S3Config {
                bucket: std::env::var("S3_BUCKET")?,
                region: std::env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string()),
                endpoint: std::env::var("S3_ENDPOINT").ok(),
                access_key: std::env::var("AWS_ACCESS_KEY_ID")?,
                secret_key: std::env::var("AWS_SECRET_ACCESS_KEY")?,
                public_url_base: std::env::var("S3_PUBLIC_URL")?,
            },
            host: std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: std::env::var("PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()?,
        })
    }
}
pub async fn app_init() -> Result<(Config, Arc<AppState>), Box<dyn Error>> {
    let config = Config::from_env()?;
    log::info!("✅ Configuration loaded");
    let ai_config = AiConfig::from_env()?;
    log::info!("✅ Ai Configuration loaded");

    // Database
    log::info!("📊 Connecting to PostgreSQL...");
    let db = setup_database(&config).await?;
    log::info!("✅ PostgreSQL connected");

    log::info!("🔄 Running migrations...");
    sqlx::migrate!("./migrations").run(&db).await?;
    log::info!("✅ Migrations completed");

    /*    // Redis
        log::info!("📮 Connecting to Redis...");
        let redis = setup_redis(&config).await?;
        log::info!("✅ Redis connected");
    */
    // S3 Storage
    log::info!("☁️  Initializing S3 with rust-s3...");
    let storage = setup_storage(&config.s3)?;
    log::info!("✅ S3 storage initialized");

    // Test S3
    match storage.list_user_images(&uuid::Uuid::now_v7()).await {
        Ok(_) => log::info!("✅ S3 connection verified"),
        Err(e) => log::warn!("⚠️  S3 test: {}", e),
    }

    // Resolvers and processors
    let image_resolver = Arc::new(ImageUrlResolver {
        storage: storage.clone(),
        db: db.clone(),
    });

    let image_processor = Arc::new(ImageProcessor::new(storage.clone()));


    let master_agent = Arc::new(MasterAgent::new(&ai_config.url));

    // Application state
    let state = Arc::new(AppState {
        db,
        storage,
        image_resolver,
        image_processor,
        master_agent,
        ai_config
    });
    Ok((config, state))
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

/*
async fn setup_redis(config: &Config) -> Result<redis::aio::ConnectionManager, redis::RedisError> {
    let client = redis::Client::open(config.redis_url.as_str())?;
    redis::aio::ConnectionManager::new(client).await
}
*/
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
