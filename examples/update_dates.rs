mod common;

use cx58_agent::db::update_leaf_datetime;
use sqlx::PgPool;
use crate::common::test_data::{generate_date, get_object3_images, get_room11_images, get_room211_images};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = PgPool::connect(&database_url).await?;

    // Get image definitions
    let room11_images = get_room11_images();
    let room211_images = get_room211_images();
    let object3_images = get_object3_images();

    // Combine all images
    let all_images: Vec<(&str, i32)> = room11_images
        .iter()
        .chain(room211_images.iter())
        .chain(object3_images.iter())
        .copied()
        .collect();

    let time = "17:00:00";

    println!("Updating dates for all leafs...\n");

    let mut updated_count = 0;
    let mut failed_count = 0;

    for (src, shift) in all_images {
        let datetime = generate_date(shift, time)?;

        match update_leaf_datetime(&pool, &src, &datetime).await {
            Ok(id) => {
                println!("  ✓ {} -> {} (id: {})", src, datetime, id);
                updated_count += 1;
            }
            Err(e) => {
                eprintln!("  ✗ {} failed: {}", src, e);
                failed_count += 1;
            }
        }
    }

    println!("\n📊 Summary:");
    println!("  Updated: {}", updated_count);
    println!("  Failed: {}", failed_count);

    if failed_count == 0 {
        println!("\n✅ All dates updated successfully");
    } else {
        println!("\n⚠️  Some updates failed");
    }

    pool.close().await;
    Ok(())
}
