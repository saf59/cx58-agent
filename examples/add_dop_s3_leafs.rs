use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{Datelike, NaiveDateTime};
use sqlx::PgPool;
use uuid::Uuid;

const DATA_DIR: &str = "data/add_data";

const ENSURE_TREE_SQL: &str = r#"
DO
$$
    DECLARE
        root_id  UUID;
        small_id UUID;
        haus_id  UUID;
        bad_id   UUID;
        kuche_id UUID;
    BEGIN
        SELECT id INTO root_id
        FROM tree_nodes
        WHERE node_type = 'Root'
        LIMIT 1;

        IF root_id IS NULL THEN
            RAISE EXCEPTION 'Root node not found in tree_nodes table';
        END IF;

        SELECT id INTO small_id
        FROM tree_nodes
        WHERE parent_id = root_id
          AND name = 'Small Objects'
          AND node_type = 'Branch'
        LIMIT 1;

        IF small_id IS NULL THEN
            INSERT INTO tree_nodes (id, parent_id, name, node_type, data)
            VALUES (uuidv7(), root_id, 'Small Objects', 'Branch', '{}'::JSONB)
            RETURNING id INTO small_id;
        END IF;

        SELECT id INTO haus_id
        FROM tree_nodes
        WHERE parent_id = small_id
          AND name = 'Haus - 01'
          AND node_type = 'Branch'
        LIMIT 1;

        IF haus_id IS NULL THEN
            INSERT INTO tree_nodes (id, parent_id, name, node_type, data)
            VALUES (uuidv7(), small_id, 'Haus - 01', 'Branch', '{}'::JSONB)
            RETURNING id INTO haus_id;
        END IF;

        SELECT id INTO bad_id
        FROM tree_nodes
        WHERE parent_id = haus_id
          AND name = 'EG - Bad'
          AND node_type = 'Branch'
        LIMIT 1;

        IF bad_id IS NULL THEN
            INSERT INTO tree_nodes (id, parent_id, name, node_type, data)
            VALUES (uuidv7(), haus_id, 'EG - Bad', 'Branch', '{}'::JSONB)
            RETURNING id INTO bad_id;
        END IF;

        SELECT id INTO kuche_id
        FROM tree_nodes
        WHERE parent_id = haus_id
          AND name = 'EG - Küche'
          AND node_type = 'Branch'
        LIMIT 1;

        IF kuche_id IS NULL THEN
            INSERT INTO tree_nodes (id, parent_id, name, node_type, data)
            VALUES (uuidv7(), haus_id, 'EG - Küche', 'Branch', '{}'::JSONB)
            RETURNING id INTO kuche_id;
        END IF;

        RAISE NOTICE 'Additional tree_nodes ensured successfully';
    END
$$;
"#;

#[derive(Debug, Clone, Copy)]
struct ImageSpec {
    node_name: &'static str,
    filename: &'static str,
}

const IMAGES: &[ImageSpec] = &[
    ImageSpec {
        node_name: "Haus - 01",
        filename: "Haus - 01 - 28.05.2026 1100.jpg",
    },
    ImageSpec {
        node_name: "EG - Bad",
        filename: "EG - Bad - 22.05.2026 1530.JPG",
    },
    ImageSpec {
        node_name: "EG - Bad",
        filename: "EG - Bad - 24.05.2026 1700.JPG",
    },
    ImageSpec {
        node_name: "EG - Bad",
        filename: "EG - Bad - 22.05.2026 1600.JPG",
    },
    ImageSpec {
        node_name: "EG - Bad",
        filename: "EG - Bad - 24.05.2026 1710.JPG",
    },
    ImageSpec {
        node_name: "EG - Bad",
        filename: "EG - Bad - 30.05.26 1800.JPG",
    },
    ImageSpec {
        node_name: "EG - Bad",
        filename: "EG - Bad - 04-06-2026 1945.JPG",
    },
    ImageSpec {
        node_name: "EG - Bad",
        filename: "EG - Bad - 07-06-2026 1900.JPG",
    },
    ImageSpec {
        node_name: "EG - Bad",
        filename: "EG - Bad - 07-06-2026 1913.JPG",
    },
    ImageSpec {
        node_name: "EG - Bad",
        filename: "EG - Bad - 16.06.2026 1800.JPG",
    },
    ImageSpec {
        node_name: "EG - Bad",
        filename: "EG - Bad - 17.06.2026 1800.JPG",
    },
    ImageSpec {
        node_name: "EG - Küche",
        filename: "EG - Küche - 23.05.2026 1800.JPG",
    },
    ImageSpec {
        node_name: "EG - Küche",
        filename: "EG - Küche - 27.05.2026 1800.JPG",
    },
];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let base_url =
        std::env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:3050".to_string());

    let pool = PgPool::connect(&database_url).await?;
    let client = reqwest::Client::new();

    println!("Ensuring tree nodes...");
    sqlx::raw_sql(ENSURE_TREE_SQL).execute(&pool).await?;

    let node_ids = load_target_node_ids(&pool).await?;
    check_files_exist()?;

    let mut images_by_node: BTreeMap<&str, Vec<&ImageSpec>> = BTreeMap::new();
    for image in IMAGES {
        images_by_node
            .entry(image.node_name)
            .or_default()
            .push(image);
    }

    for (node_name, images) in images_by_node {
        let node_id = *node_ids
            .get(node_name)
            .ok_or_else(|| format!("Node '{}' not found after tree creation", node_name))?;

        let existing_leafs = load_existing_leafs(&pool, node_id).await?;
        upload_images_for_node(
            &client,
            &base_url,
            node_id,
            node_name,
            &images,
            &existing_leafs,
        )
        .await?;
    }

    println!("\nLeafs data added successfully");

    pool.close().await;
    Ok(())
}

async fn load_target_node_ids(
    pool: &PgPool,
) -> Result<BTreeMap<&'static str, Uuid>, Box<dyn std::error::Error>> {
    let root_id: Uuid = sqlx::query_scalar(
        r#"
        SELECT id
        FROM tree_nodes
        WHERE node_type = 'Root'
        LIMIT 1
        "#,
    )
    .fetch_one(pool)
    .await?;

    let small_id = get_child_id(pool, root_id, "Small Objects").await?;
    let haus_id = get_child_id(pool, small_id, "Haus - 01").await?;
    let bad_id = get_child_id(pool, haus_id, "EG - Bad").await?;
    let kuche_id = get_child_id(pool, haus_id, "EG - Küche").await?;

    Ok(BTreeMap::from([
        ("Haus - 01", haus_id),
        ("EG - Bad", bad_id),
        ("EG - Küche", kuche_id),
    ]))
}

async fn get_child_id(
    pool: &PgPool,
    parent_id: Uuid,
    name: &str,
) -> Result<Uuid, Box<dyn std::error::Error>> {
    let id = sqlx::query_scalar(
        r#"
        SELECT id
        FROM tree_nodes
        WHERE parent_id = $1
          AND name = $2
          AND node_type = 'Branch'
        LIMIT 1
        "#,
    )
    .bind(parent_id)
    .bind(name)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

async fn load_existing_leafs(
    pool: &PgPool,
    parent_id: Uuid,
) -> Result<BTreeMap<String, Vec<Uuid>>, Box<dyn std::error::Error>> {
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        r#"
        SELECT id, data->>'src'
        FROM tree_nodes
        WHERE parent_id = $1
          AND node_type = 'ImageLeaf'
          AND data ? 'src'
        "#,
    )
    .bind(parent_id)
    .fetch_all(pool)
    .await?;

    let mut leafs = BTreeMap::new();
    for (node_id, filename) in rows {
        leafs.entry(filename).or_insert_with(Vec::new).push(node_id);
    }

    Ok(leafs)
}

fn check_files_exist() -> Result<(), Box<dyn std::error::Error>> {
    let mut missing_files = Vec::new();

    for image in IMAGES {
        let path = data_path(image.filename);
        if !path.exists() {
            missing_files.push(path.display().to_string());
        }
    }

    if missing_files.is_empty() {
        println!("All files verified successfully");
        Ok(())
    } else {
        Err(format!("Missing files:\n  - {}", missing_files.join("\n  - ")).into())
    }
}

async fn upload_images_for_node(
    client: &reqwest::Client,
    base_url: &str,
    node_id: Uuid,
    node_name: &str,
    images: &[&ImageSpec],
    existing_leafs: &BTreeMap<String, Vec<Uuid>>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("Uploading leafs for {} ({})...", node_name, node_id);

    for image in images {
        if let Some(existing_node_ids) = existing_leafs.get(image.filename) {
            for existing_node_id in existing_node_ids {
                delete_existing_leaf(client, base_url, *existing_node_id, image.filename).await?;
            }
        }

        let file_path = data_path(image.filename);
        let image_data = tokio::fs::read(&file_path).await?;
        let berlin_datetime = datetime_from_filename(image.filename)?;

        let form = reqwest::multipart::Form::new()
            .part(
                "image",
                reqwest::multipart::Part::bytes(image_data).file_name(image.filename.to_string()),
            )
            .text("berlin_datetime", berlin_datetime);

        let url = format!("{}/agent/images/upload/{}", base_url, node_id);
        let response = client.post(&url).multipart(form).send().await?;
        let status = response.status();

        if status.is_success() {
            println!("  {} uploaded", image.filename);
        } else {
            let body = response.text().await.unwrap_or_default();
            return Err(format!("{} upload failed: {} {}", image.filename, status, body).into());
        }
    }

    Ok(())
}

async fn delete_existing_leaf(
    client: &reqwest::Client,
    base_url: &str,
    node_id: Uuid,
    filename: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("{}/agent/images/{}", base_url, node_id);
    let response = client.delete(&url).send().await?;
    let status = response.status();

    if status.is_success() {
        println!("  {} deleted before refresh", filename);
        Ok(())
    } else {
        let body = response.text().await.unwrap_or_default();
        Err(format!("{} delete failed: {} {}", filename, status, body).into())
    }
}

fn datetime_from_filename(filename: &str) -> Result<String, Box<dyn std::error::Error>> {
    let stem = std::path::Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("Invalid filename: {}", filename))?;

    let (_, raw_datetime) = stem
        .rsplit_once(" - ")
        .ok_or_else(|| format!("Filename does not contain date: {}", filename))?;

    let parsed = ["%d.%m.%Y %H%M", "%d.%m.%y %H%M", "%d-%m-%Y %H%M"]
        .iter()
        .find_map(|format| NaiveDateTime::parse_from_str(raw_datetime, format).ok())
        .ok_or_else(|| format!("Unsupported filename date format: {}", filename))?;

    let parsed = if parsed.year() < 100 {
        parsed
            .with_year(parsed.year() + 2000)
            .ok_or_else(|| format!("Invalid filename year: {}", filename))?
    } else {
        parsed
    };

    if parsed.year() < 2000 || parsed.year() > 2100 {
        return Err(format!("Invalid filename year: {}", filename).into());
    }

    Ok(parsed.format("%d.%m.%Y %H:%M:%S").to_string())
}

fn data_path(filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(DATA_DIR)
        .join(filename)
}
