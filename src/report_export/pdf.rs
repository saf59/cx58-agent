use super::ReportExport;
use crate::error::{AppError, ErrorCode, Result};
use crate::localization::LocalizationManager;
use crate::power_outlets::{PowerOutletCount, PowerOutletResult};
use krilla::Document;
use krilla::color::rgb::Color;
use krilla::geom::{Point, Size, Transform};
use krilla::image::Image;
use krilla::page::PageSettings;
use krilla::paint::Fill;
use krilla::surface::Surface;
use krilla::text::{Font, TextDirection};
use unicode_segmentation::UnicodeSegmentation;

const FONT_BYTES: &[u8] = include_bytes!("fonts/NotoSans-Regular.ttf");
const WIDTH: f32 = 595.28;
const HEIGHT: f32 = 841.89;
const MARGIN: f32 = 44.0;
const BODY_WIDTH: f32 = WIDTH - 2.0 * MARGIN;
const TOP: f32 = 78.0;
const BOTTOM: f32 = 773.0;
const INK: [u8; 3] = [26, 38, 54];
const BRAND: [u8; 3] = [3, 100, 138];
const MUTED: [u8; 3] = [75, 91, 107];

enum Item {
    Text {
        y: f32,
        text: String,
        size: f32,
        color: [u8; 3],
    },
    Photo {
        y: f32,
        image: Image,
        size: Size,
    },
}

struct Layout {
    pages: Vec<Vec<Item>>,
    y: f32,
    metrics: ttf_parser::Face<'static>,
}

impl Layout {
    fn reserve(&mut self, height: f32) {
        if self.y + height > BOTTOM {
            self.pages.push(Vec::new());
            self.y = TOP;
        }
    }

    fn place(&mut self, item: Item) {
        self.pages
            .last_mut()
            .expect("layout starts with one page")
            .push(item);
    }

    fn heading(&mut self, label: &str, size: f32) {
        self.reserve(size + 32.0);
        self.paragraph(label, size, BRAND);
    }

    fn paragraph(&mut self, text: &str, size: f32, color: [u8; 3]) {
        let mut prepared = String::with_capacity(text.len());
        for ch in text.chars() {
            match ch {
                '\r' => (),
                '\t' => prepared.push_str("    "),
                '\n' => prepared.push(ch),
                _ if self.metrics.glyph_index(ch).is_some() => prepared.push(ch),
                _ => prepared.extend(ch.escape_unicode()),
            }
        }
        let metrics = &self.metrics;
        let measure = |value: &str| -> f32 {
            let units: u32 = value
                .chars()
                .filter_map(|ch| {
                    metrics
                        .glyph_index(ch)
                        .and_then(|glyph| metrics.glyph_hor_advance(glyph))
                })
                .map(u32::from)
                .sum();
            units as f32 / f32::from(metrics.units_per_em()) * size
        };
        let lines = wrap(&prepared, &measure);
        let line_height = size * 1.45;
        for line in lines {
            self.reserve(line_height);
            self.place(Item::Text {
                y: self.y + size,
                text: line,
                size,
                color,
            });
            self.y += line_height;
        }
        self.y += 6.0;
    }

    fn photo(&mut self, image: Image) -> Result<()> {
        let (width, height) = image.size();
        let max_height = (BODY_WIDTH * height as f32 / width as f32).min(300.0);
        self.reserve(max_height.min(210.0) + 10.0);
        let scale = max_height.min(BOTTOM - self.y - 10.0) / height as f32;
        let size = Size::from_wh(width as f32 * scale, height as f32 * scale)
            .ok_or_else(|| AppError::internal("Invalid report photo size"))?;
        let y = self.y;
        self.y += size.height() + 10.0;
        self.place(Item::Photo { y, image, size });
        Ok(())
    }
}

fn wrap(text: &str, measure: &impl Fn(&str) -> f32) -> Vec<String> {
    const MAX_WIDTH: f32 = BODY_WIDTH - 4.0;
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        let mut width = 0.0;
        for word in paragraph.split_word_bounds() {
            let word_width = measure(word);
            if width + word_width > MAX_WIDTH && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                width = 0.0;
            }
            if word_width <= MAX_WIDTH {
                line.push_str(word);
                width += word_width;
                continue;
            }
            for part in word.graphemes(true) {
                let part_width = measure(part);
                if width + part_width > MAX_WIDTH && !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                    width = 0.0;
                }
                line.push_str(part);
                width += part_width;
            }
        }
        lines.push(line);
    }
    lines
}

pub(super) fn render(report: &ReportExport, localization: &LocalizationManager) -> Result<Vec<u8>> {
    let font = Font::new(FONT_BYTES.to_vec().into(), 0)
        .ok_or_else(|| AppError::internal("Cannot load report font"))?;
    let metrics = ttf_parser::Face::parse(FONT_BYTES, 0)
        .map_err(|_| AppError::internal("Cannot parse report font"))?;
    let label = |key| localization.get_msg(report.language, key);
    let mut layout = Layout {
        pages: vec![Vec::new()],
        y: TOP,
        metrics,
    };
    let title = label(if report.kind == "description" {
        "report-export-description"
    } else {
        "report-export-comparison"
    });
    layout.heading(&title, 22.0);
    layout.heading(&label("report-export-tree-path"), 11.0);
    let path = report
        .tree_path
        .iter()
        .map(|node| node.name.as_str())
        .collect::<Vec<_>>()
        .join(" / ");
    layout.paragraph(&path, 11.0, INK);
    if let Some(cx_id) = &report.object_cx_id {
        layout.heading(&label("report-export-object-cx-id"), 11.0);
        layout.paragraph(cx_id, 11.0, INK);
    }
    layout.heading(&label("report-export-request"), 13.0);
    layout.paragraph(&report.request, 11.0, INK);
    layout.heading(&label("report-export-result"), 13.0);
    layout.paragraph(&report.result.description, 11.0, INK);
    for (key, value) in [
        ("report-export-windows", &report.result.windows),
        ("report-export-doors", &report.result.doors),
        ("report-export-radiators", &report.result.radiators),
        ("report-export-openings", &report.result.openings),
        ("report-export-model", &report.result.model_name),
    ] {
        if let Some(value) = value {
            layout.heading(&label(key), 11.0);
            layout.paragraph(value, 11.0, INK);
        }
    }
    if let Some(outlets) = &report.result.power_outlets {
        let display = |count: Option<PowerOutletCount>| match count {
            Some(count) if count.uncertain => format!(
                "{} ({})",
                count.confirmed,
                label("report-export-outlet-uncertain")
            ),
            Some(count) => count.confirmed.to_string(),
            None => label("report-export-outlet-unavailable"),
        };
        let text = match outlets {
            PowerOutletResult::Photo(count) => display(Some(*count)),
            PowerOutletResult::Comparison(counts) => format!(
                "{}: {}\n{}: {}",
                label("report-export-previous-photo"),
                display(counts.previous),
                label("report-export-current-photo"),
                display(counts.current),
            ),
        };
        layout.heading(&label("report-export-power-outlets"), 11.0);
        layout.paragraph(&text, 11.0, INK);
    }
    for (index, source) in report.reports.iter().enumerate() {
        let image = Image::from_jpeg(source.photo.bytes.clone().into(), true)
            .map_err(AppError::internal)?;
        let key = match (report.kind, index) {
            ("description", _) => "report-export-photo",
            (_, 0) => "report-export-previous-photo",
            _ => "report-export-current-photo",
        };
        // Keep normal photo captions together with the image when starting near a page break.
        layout.reserve(350.0);
        layout.heading(&label(key), 13.0);
        layout.paragraph(&source.name, 11.0, INK);
        let captured_at = source.captured_at.format("%d.%m.%Y %H:%M:%S %:z");
        layout.paragraph(&format!("{captured_at} (Europe/Berlin)"), 10.0, MUTED);
        if let Some(cx_id) = &source.cx_id {
            layout.heading("CX ID", 11.0);
            layout.paragraph(cx_id, 11.0, INK);
        }
        layout.photo(image)?;
    }

    let mut pdf = Document::new();
    let page_count = layout.pages.len();
    if page_count > 100 {
        return Err(AppError::new(
            ErrorCode::PayloadTooLarge,
            "PDF exceeds 100 pages",
        ));
    }
    let page_label = label("report-export-page");
    let generated = report.generated_at.format("%Y-%m-%d %H:%M:%S UTC");
    for (index, items) in layout.pages.into_iter().enumerate() {
        let mut page =
            pdf.start_page_with(PageSettings::from_wh(WIDTH, HEIGHT).expect("A4 dimensions"));
        let mut surface = page.surface();
        draw_text(
            &mut surface,
            &font,
            MARGIN,
            46.0,
            "Construct-X / CX58",
            18.0,
            BRAND,
        );
        for item in items {
            match item {
                Item::Text {
                    y,
                    text,
                    size,
                    color,
                } => {
                    draw_text(&mut surface, &font, MARGIN, y, &text, size, color);
                }
                Item::Photo { y, image, size } => {
                    surface.push_transform(&Transform::from_translate(MARGIN, y));
                    surface.draw_image(image, size);
                    surface.pop();
                }
            }
        }
        draw_text(
            &mut surface,
            &font,
            MARGIN,
            804.0,
            &report.export_id.to_string(),
            9.0,
            MUTED,
        );
        draw_text(
            &mut surface,
            &font,
            MARGIN,
            819.0,
            &generated.to_string(),
            9.0,
            MUTED,
        );
        let number = format!("{page_label} {} / {page_count}", index + 1);
        draw_text(
            &mut surface,
            &font,
            WIDTH - 105.0,
            804.0,
            &number,
            9.0,
            MUTED,
        );
        surface.finish();
        page.finish();
    }
    pdf.finish()
        .map_err(|error| AppError::internal(format!("{error:?}")))
}

fn draw_text(
    surface: &mut Surface<'_>,
    font: &Font,
    x: f32,
    y: f32,
    text: &str,
    size: f32,
    [r, g, b]: [u8; 3],
) {
    if text.is_empty() {
        return;
    }
    surface.set_fill(Some(Fill {
        paint: Color::new(r, g, b).into(),
        ..Fill::default()
    }));
    surface.draw_text(
        Point::from_xy(x, y),
        font.clone(),
        size,
        text,
        false,
        TextDirection::Auto,
    );
}
