use sqlx::PgPool;
use cx58_agent::db::{get_id_by_name, insert_image_leaf};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    let pool = PgPool::connect(&database_url).await?;

    let branch11_id_name = "Room 11";
    let branch211_id_name = "Room 211";
    let branch3_id_name = "Object 3";

    let branch11_id = get_id_by_name(&pool, branch11_id_name)
        .await?
        .ok_or(format!("Node '{}' not found", branch11_id_name))?;

    let branch211_id = get_id_by_name(&pool, branch211_id_name)
        .await?
        .ok_or(format!("Node '{}' not found", branch211_id_name))?;

    let branch3_id = get_id_by_name(&pool, branch3_id_name)
        .await?
        .ok_or(format!("Node '{}' not found", branch3_id_name))?;

    // Room 11
    insert_image_leaf(&pool, branch11_id, "4к_1.jpg", "27.11.2025 17:00:00").await?;
    insert_image_leaf(&pool, branch11_id, "4к_2.jpg", "01.12.2025 17:00:00").await?;
    insert_image_leaf(&pool, branch11_id, "4к_3.jpg", "15.12.2025 17:00:00").await?;
    insert_image_leaf(&pool, branch11_id, "4к_4.jpg", "27.12.2025 17:00:00").await?;

    // Room 211
    insert_image_leaf(&pool, branch211_id, "3w_1.jpg", "27.11.2025 17:00:00").await?;
    insert_image_leaf(&pool, branch211_id, "3w_2.jpg", "01.12.2025 17:00:00").await?;
    insert_image_leaf(&pool, branch211_id, "3w_3.jpg", "05.12.2025 17:00:00").await?;
    insert_image_leaf(&pool, branch211_id, "3w_4.jpg", "15.12.2025 17:00:00").await?;
    insert_image_leaf(&pool, branch211_id, "3w_5.jpg", "27.11.2025 17:00:00").await?;

    // Object 3
    insert_image_leaf(&pool, branch3_id, "noise_1.jpg", "27.11.2025 17:00:00").await?;
    insert_image_leaf(&pool, branch3_id, "noise_2.jpg", "27.12.2025 17:00:00").await?;

    println!("Leafs data added successfully");

    pool.close().await;
    Ok(())
}