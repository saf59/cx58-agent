mod common;

use cx58_agent::db::get_id_by_name;
use sqlx::PgPool;
use uuid::Uuid;
use crate::common::test_data::{check_files_exist, generate_date, get_object3_images, get_room11_images, get_room211_images, run_sql_script, NodeNames, DATA_DIR};

const FILL_TREE_SQL: &str = include_str!("sql/fill_tree.sql");

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    let base_url = std::env::var("BASE_URL")
        .unwrap_or_else(|_| "http://localhost:3000".to_string());

    let pool = PgPool::connect(&database_url).await?;
    let client = reqwest::Client::new();

    let node_names = NodeNames::new();

    // Attempt to get branch11_id
    let mut branch11_id = get_id_by_name(&pool, node_names.room_11).await?;

    // If not found, execute the tree creation script
    if branch11_id.is_none() {
        println!("Node '{}' not found. Running fill_tree.sql script...", node_names.room_11);
        run_sql_script(&pool, FILL_TREE_SQL).await?;
        println!("Tree creation script completed. Re-reading node IDs...");

        branch11_id = get_id_by_name(&pool, node_names.room_11).await?;
    }
    let branch11_id = branch11_id.expect("branch11_id does not exist!");

    let branch211_id = get_id_by_name(&pool, node_names.room_211)
        .await?
        .ok_or(format!("Node '{}' not found", node_names.room_211))?;

    let branch3_id = get_id_by_name(&pool, node_names.object_3)
        .await?
        .ok_or(format!("Node '{}' not found", node_names.object_3))?;

    // Get image definitions
    let room11_images = get_room11_images();
    let room211_images = get_room211_images();
    let object3_images = get_object3_images();

    // Check if all image files exist
    println!("Checking if all image files exist...");
    let mut all_images: Vec<&str> = Vec::new();
    all_images.extend(room11_images.iter().map(|(name, _)| *name));
    all_images.extend(room211_images.iter().map(|(name, _)| *name));
    all_images.extend(object3_images.iter().map(|(name, _)| *name));

    check_files_exist(DATA_DIR, &all_images)?;
    println!("All files verified successfully!");


    let time = "17:00:00";

    // Insert leafs for each node
    insert_leafs_for_node(&client, &base_url, branch11_id, node_names.room_11, &room11_images, time).await?;
    insert_leafs_for_node(&client, &base_url, branch211_id, node_names.room_211, &room211_images, time).await?;
    insert_leafs_for_node(&client, &base_url, branch3_id, node_names.object_3, &object3_images, time).await?;

    println!("\n✅ Leafs data added successfully");

    pool.close().await;
    Ok(())
}

/// Insert leafs (images) for specified node via HTTP handler
async fn insert_leafs_for_node(
    client: &reqwest::Client,
    base_url: &str,
    node_id: Uuid,
    node_name: &str,
    images: &[(&str, i32)],
    time: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Inserting leafs for {}...", node_name);

    for (filename, shift) in images {
        let file_path = format!("{}/{}", DATA_DIR, filename);

        if !std::path::Path::new(&file_path).exists() {
            eprintln!("⚠ File not found: {}", file_path);
            continue;
        }

        let image_data = tokio::fs::read(&file_path).await?;
        let berlin_datetime = generate_date(*shift, time)?;

        let form = reqwest::multipart::Form::new()
            .part("image", reqwest::multipart::Part::bytes(image_data)
                .file_name(filename.to_string()))
            .text("berlin_datetime", berlin_datetime);

        let url = format!("{}/agent/images/upload/{}", base_url, node_id);

        match client.post(&url).multipart(form).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    println!("  ✓ {} uploaded", filename);
                } else {
                    eprintln!("  ✗ {} failed: {}", filename, response.status());
                }
            }
            Err(e) => eprintln!("  ✗ {} error: {}", filename, e),
        }

    }

    Ok(())
}
