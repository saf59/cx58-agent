use std::path::PathBuf;
use sqlx::PgPool;
use cx58_agent::db::{get_id_by_name, insert_image_leaf};

const FILL_TREE_SQL: &str = include_str!("sql/fill_tree.sql");
const DATA_DIR: &str = "data";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    let pool = PgPool::connect(&database_url).await?;

    let branch11_id_name = "Room 11";
    let branch211_id_name = "Room 211";
    let branch3_id_name = "Object 3";

    // Пытаемся получить branch11_id
    let mut branch11_id = get_id_by_name(&pool, branch11_id_name).await?;

    // Если не найден, выполняем скрипт создания дерева
    if branch11_id.is_none() {
        println!("Node '{}' not found. Running fill_tree.sql script...", branch11_id_name);
        run_sql_script(&pool, FILL_TREE_SQL).await?;
        println!("Tree creation script completed. Re-reading node IDs...");

        // Читаем branch11_id снова
        branch11_id = get_id_by_name(&pool, branch11_id_name).await?;
    }
    let branch11_id = branch11_id.expect("branch11_id is not exists!\
    ");

    let branch211_id = get_id_by_name(&pool, branch211_id_name)
        .await?
        .ok_or(format!("Node '{}' not found", branch211_id_name))?;

    let branch3_id = get_id_by_name(&pool, branch3_id_name)
        .await?
        .ok_or(format!("Node '{}' not found", branch3_id_name))?;

    // Определяем изображения для каждой ноды
    let room11_images = vec![
        ("4k_1.jpg", "27.11.2025 17:00:00"),
        ("4k_2.jpg", "01.12.2025 17:00:00"),
        ("4k_3.jpg", "15.12.2025 17:00:00"),
        ("4k_4.jpg", "27.12.2025 17:00:00"),
    ];

    let room211_images = vec![
        ("3w_1.jpg", "27.11.2025 17:00:00"),
        ("3w_2.jpg", "01.12.2025 17:00:00"),
        ("3w_3.jpg", "05.12.2025 17:00:00"),
        ("3w_5.jpg", "27.11.2025 17:00:00"),
    ];

    let object3_images = vec![
        ("noise_1.jpg", "27.11.2025 17:00:00"),
//        ("noise_2.jpg", "27.12.2025 17:00:00"),
    ];

    // Проверяем наличие всех файлов перед началом вставки
    println!("Checking if all image files exist...");
    let mut all_images: Vec<&str> = Vec::new();
    all_images.extend(room11_images.iter().map(|(name, _)| *name));
    all_images.extend(room211_images.iter().map(|(name, _)| *name));
    all_images.extend(object3_images.iter().map(|(name, _)| *name));

    check_files_exist(DATA_DIR, &all_images)?;
    println!("All files verified successfully!");

    // Вставляем листья для Room 11
    println!("Inserting leafs for Room 11...");
    for (filename, datetime) in room11_images {
        let path = format!("{}/{}", DATA_DIR, filename);
        insert_image_leaf(&pool, branch11_id, &path, datetime).await?;
        println!("  ✓ {}", filename);
    }

    // Вставляем листья для Room 211
    println!("Inserting leafs for Room 211...");
    for (filename, datetime) in room211_images {
        let path = format!("{}/{}", DATA_DIR, filename);
        insert_image_leaf(&pool, branch211_id, &path, datetime).await?;
        println!("  ✓ {}", filename);
    }

    // Вставляем листья для Object 3
    println!("Inserting leafs for Object 3...");
    for (filename, datetime) in object3_images {
        let path = format!("{}/{}", DATA_DIR, filename);
        insert_image_leaf(&pool, branch3_id, &path, datetime).await?;
        println!("  ✓ {}", filename);
    }

    println!("\n✅ Leafs data added successfully");

    pool.close().await;
    Ok(())}

/// Проверяет наличие всех файлов в указанной директории
fn check_files_exist(dir: &str, filenames: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let mut missing_files = Vec::new();

    for filename in filenames {
        let path = PathBuf::from(dir).join(filename);
        if !path.exists() {
            missing_files.push(filename.to_string());
        }
    }

    if !missing_files.is_empty() {
        return Err(format!(
            "Missing files in '{}' directory:\n  - {}",
            dir,
            missing_files.join("\n  - ")
        ).into());
    }

    Ok(())
}

pub async fn run_sql_script(
    pool: &PgPool,
    sql: &str,
) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(sql)
        .execute(pool)
        .await?;

    Ok(())
}