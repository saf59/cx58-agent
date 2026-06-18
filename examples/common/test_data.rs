#![allow(unused)]
use chrono::{Duration, Local, NaiveTime};
use rand::{Rng, RngExt};
use sqlx::PgPool;
use std::path::PathBuf;

pub const DATA_DIR: &str = "data";

/// Node names used across examples
pub struct NodeNames {
    pub room_11: &'static str,
    pub room_211: &'static str,
    pub object_3: &'static str,
}

impl NodeNames {
    pub const fn new() -> Self {
        Self {
            room_11: "Room 11",
            room_211: "Room 211",
            object_3: "Object 3",
        }
    }
}

/// Image definitions with shifts in days
pub fn get_room11_images() -> Vec<(&'static str, i32)> {
    vec![
        ("4k_1.jpg", -15),
        ("4k_2.jpg", -8),
        ("4k_3.jpg", -3),
        ("4k_4.jpg", -1),
    ]
}

pub fn get_room211_images() -> Vec<(&'static str, i32)> {
    vec![
        ("3w_1.jpg", -16),
        ("3w_2.jpg", -9),
        ("3w_3.jpg", -3),
        ("3w_5.jpg", -1),
    ]
}

pub fn get_object3_images() -> Vec<(&'static str, i32)> {
    vec![("noise_1.jpg", -1)]
}

/// Generate date in format "DD.MM.YYYY HH:mm:SS"
pub fn generate_date(shift: i32, time: &str) -> Result<String, Box<dyn std::error::Error>> {
    let today = Local::now().date_naive();
    let target_date = today + Duration::days(shift as i64);

    let mut rng = rand::rng();
    let random_seconds: u32 = rng.random_range(50400..61200); // Random seconds in a day
    let rnd_time = NaiveTime::from_num_seconds_from_midnight_opt(random_seconds, 0).unwrap();
    let formatted = rnd_time.format("%H:%M:%S").to_string();

    // Check if time is valid
    //NaiveTime::parse_from_str(time, "%H:%M:%S")?;

    Ok(format!("{} {}", target_date.format("%d.%m.%Y"), rnd_time))
}

/// Check if all files exist in specified directory
pub fn check_files_exist(dir: &str, filenames: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let mut missing_files = Vec::new();

    for filename in filenames {
        let path = PathBuf::from(dir).join(filename);
        if !path.exists() {
            eprintln!("⚠ File not found: {:?}", path);
            missing_files.push(filename.to_string());
        }
    }

    if !missing_files.is_empty() {
        return Err(format!(
            "Missing files in '{}' directory:\n  - {}",
            dir,
            missing_files.join("\n  - ")
        )
        .into());
    }

    Ok(())
}

/// Run SQL script from string
pub async fn run_sql_script(pool: &PgPool, sql: &str) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(sql).execute(pool).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_date() {
        let result = generate_date(0, "17:00:00").unwrap();
        assert!(result.contains("17:00:00"));
        assert!(result.contains('.'));
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
