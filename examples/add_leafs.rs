use std::path::PathBuf;
use sqlx::PgPool;
use cx58_agent::db::{get_id_by_name, insert_image_leaf};
use chrono::{Local, Duration, NaiveTime};
use uuid::Uuid;

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
        ("4k_1.jpg", -15),
        ("4k_2.jpg", -8),
        ("4k_3.jpg", -3),
        ("4k_4.jpg", -1),
    ];

    let room211_images = vec![
        ("3w_1.jpg", -16),
        ("3w_2.jpg", -9),
        ("3w_3.jpg", -3),
        ("3w_5.jpg", -1),
    ];

    let object3_images = vec![
        ("noise_1.jpg", -1),
//        ("noise_2.jpg", -3),
    ];

    // Проверяем наличие всех файлов перед началом вставки
    println!("Checking if all image files exist...");
    let mut all_images: Vec<&str> = Vec::new();
    all_images.extend(room11_images.iter().map(|(name, _)| *name));
    all_images.extend(room211_images.iter().map(|(name, _)| *name));
    all_images.extend(object3_images.iter().map(|(name, _)| *name));

    check_files_exist(DATA_DIR, &all_images)?;
    println!("All files verified successfully!");

    let time = "17:00:00";
    // Вставляем листья для каждой ноды
    insert_leafs_for_node(&pool, branch11_id, "Room 11", &room11_images, time).await?;
    insert_leafs_for_node(&pool, branch211_id, "Room 211", &room211_images, time).await?;
    insert_leafs_for_node(&pool, branch3_id, "Object 3", &object3_images, time).await?;

    println!("\n✅ Leafs data added successfully");

    pool.close().await;
    Ok(())
}

/// Вставляет листья (изображения) для указанной ноды
async fn insert_leafs_for_node(
    pool: &PgPool,
    node_id: Uuid,
    node_name: &str,
    images: &[(&str, i32)],
    time: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Inserting leafs for {}...", node_name);

    for (filename, shift) in images {
        let datetime = generate_date(*shift, time)?;
        let path = format!("{}/{}", DATA_DIR, filename);
        insert_image_leaf(pool, node_id, &path, &datetime).await?;
        println!("  ✓ {} ({})", filename, datetime);
    }

    Ok(())
}
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
pub fn generate_date(shift: i32, time: &str) -> Result<String, Box<dyn std::error::Error>> {
    let today = Local::now().date_naive();
    let target_date = today + Duration::days(shift as i64);

    // Проверяем корректность времени
    NaiveTime::parse_from_str(time, "%H:%M:%S")?;

    Ok(format!("{} {}", target_date.format("%d.%m.%Y"), time))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_date() {
        // Проверяем формат
        let result = generate_date(0, "17:00:00").unwrap();
        assert!(result.contains("17:00:00"));

        // Проверяем что дата содержит точки
        assert!(result.contains('.'));

        // Проверяем длину (DD.MM.YYYY HH:mm:ss = 19 символов)
        assert_eq!(result.len(), 19);
    }

    #[test]
    fn test_generate_date_negative_shift() {
        let result = generate_date(-15, "17:00:00").unwrap();
        assert!(result.contains("17:00:00"));
    }

    #[test]
    fn test_generate_date_invalid_time() {
        let result = generate_date(0, "25:00:00");
        assert!(result.is_err());
    }
}