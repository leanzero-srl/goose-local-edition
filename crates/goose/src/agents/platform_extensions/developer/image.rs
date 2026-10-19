use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use image::GenericImageView;
use rmcp::model::{CallToolResult, Content};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::edit::resolve_path;

const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImageReadParams {
    /// Local file path or http(s) URL of the image to load.
    pub source: String,
    /// Optional crop rectangle in pixels. Coordinates are measured from the top-left corner.
    /// use to zoom in and get more details.
    #[serde(default)]
    pub crop: Option<CropParams>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CropParams {
    /// Left edge of the crop rectangle in pixels.
    pub x: u32,
    /// Top edge of the crop rectangle in pixels.
    pub y: u32,
    /// Width of the crop rectangle in pixels.
    pub width: u32,
    /// Height of the crop rectangle in pixels.
    pub height: u32,
}

pub struct ImageTool;

impl ImageTool {
    pub fn new() -> Self {
        Self
    }

    pub async fn image_read_with_cwd(
        &self,
        params: ImageReadParams,
        working_dir: Option<&Path>,
    ) -> CallToolResult {
        match load_image(&params, working_dir).await {
            Ok(loaded) => {
                let mut result = CallToolResult::success(vec![
                    Content::text(loaded.summary(&params.source)).with_priority(0.0),
                    Content::image(loaded.data, loaded.mime_type.clone()).with_priority(0.0),
                ]);
                result.structured_content = Some(json!({
                    "source": params.source,
                    "mimeType": loaded.mime_type,
                    "width": loaded.width,
                    "height": loaded.height,
                    "bytes": loaded.bytes_len,
                    "originalWidth": loaded.original_width,
                    "originalHeight": loaded.original_height,
                    "crop": params.crop,
                }));
                result
            }
            Err(error) => CallToolResult::error(vec![
                Content::text(format!("Error: {error}")).with_priority(0.0)
            ]),
        }
    }
}

impl Default for ImageTool {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
struct LoadedImage {
    data: String,
    mime_type: String,
    bytes_len: usize,
    width: u32,
    height: u32,
    original_width: u32,
    original_height: u32,
    cropped: bool,
}

impl LoadedImage {
    fn summary(&self, source: &str) -> String {
        let crop_note = if self.cropped {
            format!(
                " Cropped from {}x{} to {}x{}.",
                self.original_width, self.original_height, self.width, self.height
            )
        } else {
            String::new()
        };

        format!(
            "Loaded image from {source} ({} bytes, {}, {}x{}).{crop_note}",
            self.bytes_len, self.mime_type, self.width, self.height
        )
    }
}

async fn load_image(
    params: &ImageReadParams,
    working_dir: Option<&Path>,
) -> Result<LoadedImage, String> {
    if params.source.trim().is_empty() {
        return Err("source cannot be empty".to_string());
    }

    let bytes = load_image_bytes(&params.source, working_dir).await?;
    ensure_image_size(bytes.len() as u64)?;

    let format = image::guess_format(&bytes)
        .map_err(|_| unsupported_format_message(&bytes, &params.source))?;
    let mime_type = mime_type(format)?;
    let image = image::load_from_memory_with_format(&bytes, format)
        .map_err(|error| format!("failed to decode image: {error}"))?;
    let (original_width, original_height) = image.dimensions();

    let Some(crop) = &params.crop else {
        return Ok(LoadedImage {
            data: base64::prelude::BASE64_STANDARD.encode(&bytes),
            mime_type: mime_type.to_string(),
            bytes_len: bytes.len(),
            width: original_width,
            height: original_height,
            original_width,
            original_height,
            cropped: false,
        });
    };

    validate_crop(crop, original_width, original_height)?;
    let cropped = image.crop_imm(crop.x, crop.y, crop.width, crop.height);
    let mut cropped_bytes = Cursor::new(Vec::new());
    cropped
        .write_to(&mut cropped_bytes, image::ImageFormat::Png)
        .map_err(|error| format!("failed to encode cropped image: {error}"))?;
    let cropped_bytes = cropped_bytes.into_inner();
    ensure_image_size(cropped_bytes.len() as u64)?;

    Ok(LoadedImage {
        data: base64::prelude::BASE64_STANDARD.encode(&cropped_bytes),
        mime_type: "image/png".to_string(),
        bytes_len: cropped_bytes.len(),
        width: crop.width,
        height: crop.height,
        original_width,
        original_height,
        cropped: true,
    })
}

const UNSUPPORTED_FORMAT: &str =
    "unsupported image format; supported formats are png, jpeg, gif, and webp";

/// A model that reaches for `read_image` to read a text file (a contract, a manifest, JSON) gets an
/// error it cannot act on and retries the same call until the duplicate guard stops it — Xing 4.0
/// spent 121 of its Forge 2.0 calls this way (RUN-ISSUES I-15). When the bytes are text, say so and
/// name the tool that reads text.
fn unsupported_format_message(bytes: &[u8], source: &str) -> String {
    if looks_like_text(bytes) {
        format!(
            "{UNSUPPORTED_FORMAT}. `{source}` is a text file, not an image: read it with the `shell` tool instead (for example `cat '{source}'` or `sed -n '1,200p' '{source}'`)."
        )
    } else {
        UNSUPPORTED_FORMAT.to_string()
    }
}

fn looks_like_text(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(8192)];
    !head.is_empty() && !head.contains(&0) && std::str::from_utf8(head).is_ok()
}

async fn load_image_bytes(source: &str, working_dir: Option<&Path>) -> Result<Vec<u8>, String> {
    if let Ok(url) = url::Url::parse(source) {
        match url.scheme() {
            "http" | "https" => load_url_bytes(url).await,
            "file" => {
                let path = url
                    .to_file_path()
                    .map_err(|_| "invalid file URL".to_string())?;
                load_file_bytes(path)
            }
            _ => load_file_bytes(resolve_path(source, working_dir)),
        }
    } else {
        load_file_bytes(resolve_path(source, working_dir))
    }
}

async fn load_url_bytes(url: url::Url) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!(
            "goose/",
            env!("CARGO_PKG_VERSION"),
            " (+https://github.com/aaif-goose/goose)"
        ))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| format!("failed to create HTTP client: {error}"))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("failed to download image: {error}"))?
        .error_for_status()
        .map_err(|error| format!("failed to download image: {error}"))?;

    if let Some(len) = response.content_length() {
        ensure_image_size(len)?;
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("failed to read image response: {error}"))?;

    Ok(bytes.to_vec())
}

fn load_file_bytes(path: PathBuf) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| format!("failed to read image file: {error}"))
}

fn validate_crop(crop: &CropParams, image_width: u32, image_height: u32) -> Result<(), String> {
    if crop.width == 0 || crop.height == 0 {
        return Err("crop width and height must be greater than zero".to_string());
    }

    let right = crop
        .x
        .checked_add(crop.width)
        .ok_or_else(|| "crop rectangle is out of bounds".to_string())?;
    let bottom = crop
        .y
        .checked_add(crop.height)
        .ok_or_else(|| "crop rectangle is out of bounds".to_string())?;

    if right > image_width || bottom > image_height {
        return Err(format!(
            "crop rectangle {}x{} at {},{} exceeds image bounds {}x{}",
            crop.width, crop.height, crop.x, crop.y, image_width, image_height
        ));
    }

    Ok(())
}

fn ensure_image_size(len: u64) -> Result<(), String> {
    if len > MAX_IMAGE_BYTES {
        Err(format!(
            "image is too large: {len} bytes exceeds {MAX_IMAGE_BYTES} byte limit"
        ))
    } else {
        Ok(())
    }
}

fn mime_type(format: image::ImageFormat) -> Result<&'static str, String> {
    match format {
        image::ImageFormat::Png => Ok("image/png"),
        image::ImageFormat::Jpeg => Ok("image/jpeg"),
        image::ImageFormat::Gif => Ok("image/gif"),
        image::ImageFormat::WebP => Ok("image/webp"),
        _ => Err(UNSUPPORTED_FORMAT.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_file_error_points_to_shell() {
        let msg = unsupported_format_message(
            b"# forge-2.0 contract\nmodules:\n",
            "/w/FORGE2-CONTRACT.md",
        );
        assert!(msg.starts_with(UNSUPPORTED_FORMAT));
        assert!(msg.contains("is a text file"));
        assert!(msg.contains("`shell`"));
        assert!(msg.contains("cat '/w/FORGE2-CONTRACT.md'"));
    }

    #[test]
    fn binary_non_image_keeps_the_plain_error() {
        let msg = unsupported_format_message(&[0u8, 159, 146, 150, 0, 1, 2], "/w/blob.bin");
        assert_eq!(msg, UNSUPPORTED_FORMAT);
    }

    #[test]
    fn empty_file_is_not_called_text() {
        assert_eq!(
            unsupported_format_message(b"", "/w/empty"),
            UNSUPPORTED_FORMAT
        );
    }
}
