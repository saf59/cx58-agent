mod pdf;

use crate::AppState;
use crate::db::NodeType;
use crate::error::{AppError, ErrorCode, Result};
use crate::power_outlets::PowerOutletResult;
use axum::Json;
use axum::extract::State;
use axum::http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::response::IntoResponse;
use base64::engine::general_purpose::STANDARD;
use bytes::Bytes;
use chrono::{DateTime, FixedOffset, NaiveDateTime, Utc};
use chrono_tz::Europe::Berlin;
use image::ImageDecoder;
use serde::{Deserialize, Serialize, Serializer};
use std::io::Cursor;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::sync::Semaphore;
use uuid::Uuid;

const MAX_TEXT_BYTES: usize = 128 * 1024;
const MAX_PHOTO_BYTES: usize = 10 * 1024 * 1024;
// An owned permit keeps the slot reserved if the HTTP timeout fires during rendering.
static EXPORT_SLOTS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(4)));

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Format {
    Pdf,
    Json,
}

impl Format {
    fn download_headers(&self) -> (&'static str, &'static str) {
        match self {
            Self::Pdf => (
                "application/pdf",
                "attachment; filename=\"cx58-report.pdf\"",
            ),
            Self::Json => (
                "application/json",
                "attachment; filename=\"cx58-report.json\"",
            ),
        }
    }
}

#[derive(Deserialize)]
pub struct ExportRequest {
    user_id: String,
    format: Format,
    request: String,
    language: String,
    sources: Sources,
    result: ReportResult,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Sources {
    Description { report_id: Uuid },
    Comparison { previous_id: Uuid, current_id: Uuid },
}

#[derive(Deserialize, Serialize)]
struct ReportResult {
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    windows: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    doors: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    radiators: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    openings: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    power_outlets: Option<PowerOutletResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_name: Option<String>,
}

#[derive(Serialize)]
struct ReportExport {
    schema: &'static str,
    export_id: Uuid,
    generated_at: DateTime<Utc>,
    language: &'static str,
    kind: &'static str,
    request: String,
    result: ReportResult,
    tree_path: Vec<PathNode>,
    object_cx_id: Option<String>,
    reports: Vec<Report>,
}

#[derive(Serialize)]
struct PathNode {
    id: Uuid,
    name: String,
}

#[derive(Serialize)]
struct Report {
    id: Uuid,
    name: String,
    cx_id: Option<String>,
    captured_at: DateTime<FixedOffset>,
    photo: Photo,
}

#[derive(Serialize)]
struct Photo {
    media_type: &'static str,
    #[serde(rename = "base64", serialize_with = "serialize_photo")]
    bytes: Vec<u8>,
}

fn serialize_photo<S: Serializer>(
    bytes: &[u8],
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.collect_str(&base64::display::Base64Display::new(bytes, &STANDARD))
}

#[derive(sqlx::FromRow)]
struct ExportNode {
    id: Uuid,
    parent_id: Option<Uuid>,
    node_type: NodeType,
    name: String,
    own: bool,
    cx_id: Option<String>,
    storage_path: Option<String>,
    updated_at: NaiveDateTime,
}

pub async fn download(
    State(state): State<Arc<AppState>>,
    Json(input): Json<ExportRequest>,
) -> Result<impl IntoResponse> {
    let (content_type, disposition) = input.format.download_headers();
    let permit = EXPORT_SLOTS
        .clone()
        .try_acquire_owned()
        .map_err(|_| AppError::rate_limit())?;
    let content = tokio::time::timeout(Duration::from_secs(35), async {
        let (format, photos, mut export) = assemble(&state, input).await?;
        let master_agent = state.master_agent.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            for (report, image) in export.reports.iter_mut().zip(photos) {
                report.photo.bytes = prepare_photo(image)?;
            }
            match format {
                Format::Pdf => pdf::render(&export, master_agent.localization()),
                Format::Json => serde_json::to_vec_pretty(&export)
                    .map_err(|error| AppError::internal(error.to_string())),
            }
        })
        .await
        .map_err(|error| AppError::internal(error.to_string()))?
    })
    .await
    .map_err(|_| AppError::service_unavailable("Report export"))??;

    Ok((
        [
            (CONTENT_TYPE, content_type),
            (CONTENT_DISPOSITION, disposition),
            (CACHE_CONTROL, "no-store"),
        ],
        content,
    ))
}

async fn assemble(
    state: &AppState,
    input: ExportRequest,
) -> Result<(Format, Vec<Bytes>, ReportExport)> {
    let (kind, report_ids) = match input.sources {
        Sources::Description { report_id } => ("description", vec![report_id]),
        Sources::Comparison {
            previous_id,
            current_id,
        } => ("comparison", vec![previous_id, current_id]),
    };
    let nodes = sqlx::query_as::<_, ExportNode>(
        r#"
        WITH RECURSIVE visible AS (
            SELECT id, parent_id, node_type, name, own, updated_at
            FROM get_tree($1, true)
        ), lineage AS (
            SELECT * FROM visible
            WHERE id = ANY($2) AND own AND node_type = 'ImageLeaf'
            UNION
            SELECT parent.* FROM visible parent JOIN lineage child ON child.parent_id = parent.id
        )
        SELECT node.id, node.parent_id, node.node_type, COALESCE(node.name, '') AS name,
               node.own, CASE WHEN node.own THEN stored.cx_id END AS cx_id,
               stored.data->>'storage_path' AS storage_path, node.updated_at
        FROM lineage node JOIN tree_nodes stored ON stored.id = node.id
        "#,
    )
    .bind(&input.user_id)
    .bind(&report_ids)
    .fetch_all(&state.db)
    .await?;

    let reports = report_ids
        .iter()
        .map(|id| {
            nodes
                .iter()
                .find(|node| node.id == *id && node.own && node.node_type == NodeType::ImageLeaf)
                .ok_or_else(|| AppError::not_found("Report"))
        })
        .collect::<Result<Vec<_>>>()?;
    let object_id = reports[0].parent_id;
    if reports.iter().any(|report| report.parent_id != object_id) {
        return Err(AppError::bad_request(
            "Reports must belong to the same object",
        ));
    }
    let object = nodes
        .iter()
        .find(|node| Some(node.id) == object_id && node.node_type == NodeType::Branch)
        .ok_or_else(|| AppError::not_found("Report object"))?;
    let mut tree_path = Vec::new();
    let mut ancestor = Some(object);
    while let Some(node) = ancestor {
        if tree_path.len() >= nodes.len() {
            return Err(AppError::internal("Invalid report tree"));
        }
        tree_path.push(PathNode {
            id: node.id,
            name: node.name.clone(),
        });
        ancestor = nodes
            .iter()
            .find(|parent| Some(parent.id) == node.parent_id);
    }
    tree_path.reverse();

    let result_size: usize = [
        input.result.windows.as_deref(),
        input.result.doors.as_deref(),
        input.result.radiators.as_deref(),
        input.result.openings.as_deref(),
        input.result.model_name.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::len)
    .sum();
    let metadata_size: usize = nodes
        .iter()
        .map(|node| node.name.len() + node.cx_id.as_ref().map_or(0, String::len))
        .sum();
    if input.request.len() + input.result.description.len() + result_size + metadata_size
        > MAX_TEXT_BYTES
    {
        return Err(AppError::new(
            ErrorCode::PayloadTooLarge,
            "Report text is too large",
        ));
    }

    let mut photos = Vec::with_capacity(reports.len());
    for node in &reports {
        let storage_path = node
            .storage_path
            .as_deref()
            .filter(|path| !path.is_empty())
            .ok_or_else(|| AppError::service_unavailable("Report photo"))?;
        photos.push(
            state
                .storage
                .download_image_bounded(storage_path, MAX_PHOTO_BYTES)
                .await?,
        );
    }
    let reports = reports
        .into_iter()
        .map(|node| Report {
            id: node.id,
            name: node.name.clone(),
            cx_id: node.cx_id.clone(),
            captured_at: node
                .updated_at
                .and_utc()
                .with_timezone(&Berlin)
                .fixed_offset(),
            photo: Photo {
                media_type: "image/jpeg",
                bytes: Vec::new(),
            },
        })
        .collect();
    let export = ReportExport {
        schema: "cx-report-export/v1",
        export_id: Uuid::now_v7(),
        generated_at: Utc::now(),
        language: if input.language == "de" { "de" } else { "en" },
        kind,
        request: input.request,
        result: input.result,
        tree_path,
        object_cx_id: object.cx_id.clone(),
        reports,
    };
    Ok((input.format, photos, export))
}

fn prepare_photo(image: Bytes) -> Result<Vec<u8>> {
    let invalid_photo = |_| AppError::service_unavailable("Report photo");
    let mut reader = image::ImageReader::new(Cursor::new(image))
        .with_guessed_format()
        .map_err(invalid_photo)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| AppError::service_unavailable("Report photo"))?;
    let orientation = decoder
        .orientation()
        .map_err(|_| AppError::service_unavailable("Report photo"))?;
    let mut image = image::DynamicImage::from_decoder(decoder)
        .map_err(|_| AppError::service_unavailable("Report photo"))?;
    image.apply_orientation(orientation);
    let resized = image.thumbnail(1600, 1600);
    crate::storage::encode_jpeg(&resized, 84)
        .map_err(|_| AppError::service_unavailable("Report photo"))
}
