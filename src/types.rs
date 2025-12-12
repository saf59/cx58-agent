// shared/src/types.rs

use serde::{Deserialize, Serialize};
use std::fmt;

// ============================================================================
// Language Support
// ============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Language {
    #[serde(rename = "en")]
    English,
    #[serde(rename = "uk")]
    Ukrainian,
    #[serde(rename = "de")]
    German,
}

impl Language {
    pub fn from_code(code: &str) -> Self {
        match code.to_lowercase().as_str() {
            "uk" => Self::Ukrainian,
            "de" => Self::German,
            _ => Self::English,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Ukrainian => "uk",
            Self::German => "de",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Ukrainian => "Українська",
            Self::German => "Deutsch",
        }
    }
}

impl Default for Language {
    fn default() -> Self {
        Self::English
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code())
    }
}

// ============================================================================
// File Size Helper
// ============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileSize(pub u64);

impl FileSize {
    pub const fn bytes(size: u64) -> Self {
        Self(size)
    }

    pub const fn kilobytes(size: u64) -> Self {
        Self(size * 1024)
    }

    pub const fn megabytes(size: u64) -> Self {
        Self(size * 1024 * 1024)
    }

    pub const fn gigabytes(size: u64) -> Self {
        Self(size * 1024 * 1024 * 1024)
    }

    pub fn as_bytes(&self) -> u64 {
        self.0
    }

    pub fn as_kilobytes(&self) -> f64 {
        self.0 as f64 / 1024.0
    }

    pub fn as_megabytes(&self) -> f64 {
        self.0 as f64 / (1024.0 * 1024.0)
    }

    pub fn as_gigabytes(&self) -> f64 {
        self.0 as f64 / (1024.0 * 1024.0 * 1024.0)
    }

    pub fn human_readable(&self) -> String {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = self.0 as f64;
        let mut unit_idx = 0;

        while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
            size /= 1024.0;
            unit_idx += 1;
        }

        format!("{:.1} {}", size, UNITS[unit_idx])
    }
}

impl fmt::Display for FileSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.human_readable())
    }
}

impl From<u64> for FileSize {
    fn from(size: u64) -> Self {
        Self(size)
    }
}

// ============================================================================
// MIME Type Helper
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MimeType(String);

impl MimeType {
    pub fn new(mime: impl Into<String>) -> Self {
        Self(mime.into())
    }

    pub fn is_image(&self) -> bool {
        self.0.starts_with("image/")
    }

    pub fn is_jpeg(&self) -> bool {
        self.0 == "image/jpeg"
    }

    pub fn is_png(&self) -> bool {
        self.0 == "image/png"
    }

    pub fn is_gif(&self) -> bool {
        self.0 == "image/gif"
    }

    pub fn is_webp(&self) -> bool {
        self.0 == "image/webp"
    }

    pub fn extension(&self) -> Option<&str> {
        match self.0.as_str() {
            "image/jpeg" => Some("jpg"),
            "image/png" => Some("png"),
            "image/gif" => Some("gif"),
            "image/webp" => Some("webp"),
            "image/svg+xml" => Some("svg"),
            "image/bmp" => Some("bmp"),
            "image/tiff" => Some("tiff"),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MimeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for MimeType {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for MimeType {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

// ============================================================================
// Timestamp Helper
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Timestamp(String);

impl Timestamp {
    pub fn now() -> Self {
        Self(chrono::Utc::now().to_rfc3339())
    }

    pub fn from_string(s: String) -> Self {
        Self(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn format_relative(&self) -> String {
        "just now".to_string()
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for Timestamp {
    fn from(s: String) -> Self {
        Self(s)
    }
}

// ============================================================================
// URL Validation
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Url(String);

impl Url {
    pub fn new(url: impl Into<String>) -> Result<Self, String> {
        let url = url.into();
        if Self::is_valid(&url) {
            Ok(Self(url))
        } else {
            Err("Invalid URL".to_string())
        }
    }

    pub fn is_valid(url: &str) -> bool {
        url.starts_with("http://") || url.starts_with("https://")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn domain(&self) -> Option<String> {
        self.0
            .strip_prefix("https://")
            .or_else(|| self.0.strip_prefix("http://"))
            .and_then(|s| s.split('/').next())
            .map(|s| s.to_string())
    }
}

impl fmt::Display for Url {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ============================================================================
// Color Helper (for UI)
// ============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn to_hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    pub fn to_rgba(&self) -> String {
        format!(
            "rgba({}, {}, {}, {})",
            self.r,
            self.g,
            self.b,
            self.a as f32 / 255.0
        )
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::rgb(0, 0, 0)
    }
}

// ============================================================================
// Confidence Score
// ============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, PartialOrd)]
pub struct Confidence(f32);

impl Confidence {
    pub fn new(value: f32) -> Result<Self, String> {
        if (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err("Confidence must be between 0.0 and 1.0".to_string())
        }
    }

    pub fn value(&self) -> f32 {
        self.0
    }

    pub fn percentage(&self) -> f32 {
        self.0 * 100.0
    }

    pub fn is_high(&self) -> bool {
        self.0 >= 0.8
    }

    pub fn is_medium(&self) -> bool {
        self.0 >= 0.5 && self.0 < 0.8
    }

    pub fn is_low(&self) -> bool {
        self.0 < 0.5
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.1}%", self.percentage())
    }
}

// ============================================================================
// Validation Helpers
// ============================================================================

pub trait Validate {
    fn validate(&self) -> Result<(), String>;
}

pub fn validate_email(email: &str) -> bool {
    email.contains('@') && email.contains('.')
}

pub fn validate_uuid(s: &str) -> bool {
    uuid::Uuid::parse_str(s).is_ok()
}

// ============================================================================
// Range Helper
// ============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Range<T> {
    pub min: T,
    pub max: T,
}

impl<T: PartialOrd> Range<T> {
    pub fn new(min: T, max: T) -> Result<Self, String> {
        if min <= max {
            Ok(Self { min, max })
        } else {
            Err("Min must be less than or equal to max".to_string())
        }
    }

    pub fn contains(&self, value: &T) -> bool {
        value >= &self.min && value <= &self.max
    }
}

// ============================================================================
// Feature Flags
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Features {
    pub vision_enabled: bool,
    pub batch_upload_enabled: bool,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            vision_enabled: true,
            batch_upload_enabled: true,
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_size() {
        let size = FileSize::megabytes(5);
        assert_eq!(size.as_bytes(), 5 * 1024 * 1024);
        assert!(size.human_readable().contains("MB"));
    }

    #[test]
    fn test_language() {
        let lang = Language::from_code("uk");
        assert_eq!(lang, Language::Ukrainian);
        assert_eq!(lang.code(), "uk");
    }

    #[test]
    fn test_mime_type() {
        let mime = MimeType::new("image/jpeg");
        assert!(mime.is_image());
        assert!(mime.is_jpeg());
        assert_eq!(mime.extension(), Some("jpg"));
    }

    #[test]
    fn test_color() {
        let color = Color::rgb(255, 0, 0);
        assert_eq!(color.to_hex(), "#ff0000");
    }

    #[test]
    fn test_confidence() {
        let conf = Confidence::new(0.85).unwrap();
        assert!(conf.is_high());
        assert_eq!(conf.percentage(), 85.0);
    }

    #[test]
    fn test_url_validation() {
        assert!(Url::is_valid("https://example.com"));
        assert!(!Url::is_valid("not-a-url"));
    }

    #[test]
    fn test_range() {
        let range = Range::new(0, 100).unwrap();
        assert!(range.contains(&50));
        assert!(!range.contains(&150));
    }
}
