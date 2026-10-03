use std::{borrow::Cow, io::Read as _, path::Path};

use base64::Engine as _;
use rmcp::model::{AnnotateAble as _, ImageContent, RawImageContent};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::base::MessageStream;
use crate::conversation::message::{Message, SystemNotificationType};
use crate::errors::ProviderError;

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum ImageFormat {
    OpenAi,
    Anthropic,
}

/// Convert an image content into an image json based on format
pub fn convert_image(image: &ImageContent, image_format: &ImageFormat) -> Value {
    match image_format {
        ImageFormat::OpenAi => json!({
            "type": "image_url",
            "image_url": {
                "url": format!("data:{};base64,{}", image.mime_type, image.data)
            }
        }),
        ImageFormat::Anthropic => json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": image.mime_type,
                "data": image.data,
            }
        }),
    }
}

/// An image part a request to a text-only engine did not carry (Q-260). `label` names it in the
/// placeholder the model reads and in the chat's notice; `new` = no assistant message follows it
/// in the request, so this is the first request that withholds it and the chat has not been told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WithheldImage {
    pub label: String,
    pub new: bool,
}

/// What formatting a request did with its image parts: how many went out as images, and which
/// were withheld because the engine reads text only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageReport {
    pub sent: usize,
    pub withheld: Vec<WithheldImage>,
}

/// The label of an image a user message names by its path: the file's name.
pub fn path_image_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// The label of an image attached to a message (a paste or a dropped file carries no name).
pub fn attached_image_label(image: &ImageContent) -> String {
    format!("attachment ({})", image.mime_type)
}

/// The label of an image a tool returned, by the tool that returned it when the request names it.
pub fn tool_image_label(tool: Option<&str>, mime_type: &str) -> String {
    match tool {
        Some(tool) => format!("from {tool} ({mime_type})"),
        None => format!("from a tool result ({mime_type})"),
    }
}

/// The text a text-only engine reads where the image was.
pub fn withheld_image_placeholder(label: &str) -> String {
    format!("[image {label} not sent: this model reads text only]")
}

/// The chat's one notice for the images this request is the first to withhold; `None` when every
/// withheld image was already announced by an earlier request (a past image in history).
pub fn withheld_images_notice(model: &str, withheld: &[WithheldImage]) -> Option<String> {
    let labels: Vec<&str> = withheld
        .iter()
        .filter(|image| image.new)
        .map(|image| image.label.as_str())
        .collect();
    let model = model.rsplit('/').next().unwrap_or(model);
    match labels.as_slice() {
        [] => None,
        [one] => Some(format!(
            "{model} reads text only — the image {one} was not sent"
        )),
        many => Some(format!(
            "{model} reads text only — the images {} were not sent",
            many.join(", ")
        )),
    }
}

/// What a model's provider declares about image input. A provider reads it from its own metadata:
/// a goose MLX engine's `/v1/models` `capabilities` (Q-260), OpenRouter's models listing
/// `architecture.input_modalities`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageInput {
    /// The model takes image parts.
    Reads,
    /// The model takes text only: the upstream refuses image parts (OpenRouter answers 404 "No
    /// endpoints found that support image input"), and a refused image left in history would fail
    /// every later turn of the session.
    TextOnly,
    /// The metadata says nothing either way (no entry for the model, or no such field).
    Undeclared,
}

/// The stream, led by the chat's notice when this request is the first to withhold an image
/// (Q-260). The notice is a system notification — no formatter sends it to a model — so it is
/// shown and kept in the session, never read back as something the model said. It rides in front
/// of the stream's FIRST item and only when that item is not an error: a stream that fails before
/// its first item stays retryable (`reply_parts`' retry-before-the-first-item contract), and the
/// retried request carries the notice again.
pub fn announced(stream: MessageStream, notice: Option<String>) -> MessageStream {
    use futures::StreamExt;
    let Some(notice) = notice else {
        return stream;
    };
    let mut notice = Some(
        Message::assistant()
            .with_system_notification(SystemNotificationType::InlineMessage, notice),
    );
    Box::pin(stream.flat_map(move |item| {
        let lead = notice
            .take()
            .filter(|_| item.is_ok())
            .map(|message| Ok((Some(message), None)));
        futures::stream::iter(lead.into_iter().chain(std::iter::once(item)))
    }))
}

pub fn detect_image_path(text: &str) -> Option<Cow<'_, str>> {
    const EXTENSIONS: [&str; 3] = [".png", ".jpg", ".jpeg"];
    const MAX_PATH_LEN: usize = 4096;

    let mut best: Option<(usize, Cow<'_, str>)> = None;
    let mut from = 0;
    while from < text.len() {
        let Some(end) = EXTENSIONS
            .iter()
            .filter_map(|ext| find_ascii_ci(text, ext, from).map(|i| i + ext.len()))
            .min()
        else {
            break;
        };

        let terminator = text.get(end..).and_then(|rest| rest.chars().next());
        let terminated = terminator.is_none_or(is_path_terminator);

        if terminated {
            let mut floor = end.saturating_sub(MAX_PATH_LEN);
            while floor < end && !text.is_char_boundary(floor) {
                floor += 1;
            }
            if let Some(window) = text.get(floor..end) {
                for (rel, _) in window.match_indices('/') {
                    let start = floor + rel;
                    let preceded_by_boundary = text
                        .get(..start)
                        .and_then(|prefix| prefix.chars().next_back())
                        .is_none_or(is_path_leading_boundary);
                    if !preceded_by_boundary {
                        continue;
                    }
                    let Some(candidate) = text.get(start..end) else {
                        continue;
                    };
                    if let Some(candidate_path) = image_path_candidate(candidate) {
                        // Keep the first referenced path, but allow a longer
                        // match anchored at the same start to extend it (a
                        // whitespace-terminated extension may be a prefix of a
                        // spaced filename ending in a later extension).
                        match best {
                            Some((best_start, _)) if start == best_start => {
                                best = Some((start, candidate_path));
                            }
                            None => best = Some((start, candidate_path)),
                            Some(_) => {}
                        }
                        break;
                    }
                }
            }
        }
        from = end;
    }
    best.map(|(_, candidate)| candidate)
}

fn clean_path(path: &str) -> Cow<'_, str> {
    if !path.contains('\\') {
        return Cow::Borrowed(path);
    }

    let mut cleaned = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();
    let mut changed = false;

    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                if !next.is_alphanumeric() {
                    cleaned.push(next);
                    chars.next();
                    changed = true;
                    continue;
                }
            }
        }
        cleaned.push(c);
    }

    if changed {
        Cow::Owned(cleaned)
    } else {
        Cow::Borrowed(path)
    }
}

fn image_path_candidate(candidate: &str) -> Option<Cow<'_, str>> {
    if is_existing_image_path(candidate) {
        return Some(Cow::Borrowed(candidate));
    }

    let cleaned = clean_path(candidate);
    if cleaned.as_ref() != candidate && is_existing_image_path(cleaned.as_ref()) {
        return Some(cleaned);
    }

    None
}

fn is_existing_image_path(candidate: &str) -> bool {
    let path = Path::new(candidate);
    path.is_absolute() && path.is_file() && is_image_file(path)
}

fn is_path_leading_boundary(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | '\u{00AB}' | '\u{00BB}' | '\u{2018}'
                ..='\u{201F}' | '\u{2039}' | '\u{203A}'
        )
}

fn is_path_terminator(c: char) -> bool {
    c == '/'
        || c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | '\u{00AB}' | '\u{00BB}' | '\u{2013}'
                ..='\u{201F}' | '\u{2026}' | '\u{2039}' | '\u{203A}'
        )
        || ('\u{2300}'..='\u{23FF}').contains(&c)
        || ('\u{2600}'..='\u{27BF}').contains(&c)
        || ('\u{2B00}'..='\u{2BFF}').contains(&c)
        || ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
        || ('\u{1F300}'..='\u{1FAFF}').contains(&c)
}

/// Case-insensitive ASCII substring search returning a byte index into
/// `haystack` (no allocation, so the index stays valid for slicing).
fn find_ascii_ci(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    let (hb, nb) = (haystack.as_bytes(), needle.as_bytes());
    if nb.is_empty() || hb.len() < nb.len() || from > hb.len() - nb.len() {
        return None;
    }
    (from..=hb.len() - nb.len()).find(|&i| {
        hb[i..i + nb.len()]
            .iter()
            .zip(nb)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
    })
}

/// Check if a file is actually an image by examining its magic bytes
fn is_image_file(path: &Path) -> bool {
    if let Ok(mut file) = std::fs::File::open(path) {
        let mut buffer = [0u8; 8]; // Large enough for most image magic numbers
        if file.read(&mut buffer).is_ok() {
            // Check magic numbers for common image formats
            return match &buffer[0..4] {
                // PNG: 89 50 4E 47
                [0x89, 0x50, 0x4E, 0x47] => true,
                // JPEG: FF D8 FF
                [0xFF, 0xD8, 0xFF, _] => true,
                // GIF: 47 49 46 38
                [0x47, 0x49, 0x46, 0x38] => true,
                _ => false,
            };
        }
    }
    false
}

/// Convert a local image file to base64 encoded ImageContent
pub fn load_image_file(path: &str) -> Result<ImageContent, ProviderError> {
    let path = Path::new(path);

    // Verify it's an image before proceeding
    if !is_image_file(path) {
        return Err(ProviderError::RequestFailed(
            "File is not a valid image".to_string(),
        ));
    }

    // Read the file
    let bytes = std::fs::read(path)
        .map_err(|e| ProviderError::RequestFailed(format!("Failed to read image file: {}", e)))?;

    // Detect mime type from extension
    let mime_type = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => match ext.to_lowercase().as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            _ => {
                return Err(ProviderError::RequestFailed(
                    "Unsupported image format".to_string(),
                ))
            }
        },
        None => {
            return Err(ProviderError::RequestFailed(
                "Unknown image format".to_string(),
            ))
        }
    };

    // Convert to base64
    let data = base64::prelude::BASE64_STANDARD.encode(&bytes);

    Ok(RawImageContent {
        mime_type: mime_type.to_string(),
        data,
        meta: None,
    }
    .no_annotation())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_names_the_model_and_only_the_images_not_yet_announced() {
        let image = |label: &str, new: bool| WithheldImage {
            label: label.to_string(),
            new,
        };
        let model = "Mihai-LeanZero/Qwen3.8-27B-Atlassian-Q8-mlx";
        assert_eq!(
            withheld_images_notice(model, &[image("old.png", false)]),
            None,
            "a past image in history is never announced again"
        );
        assert_eq!(
            withheld_images_notice(model, &[image("old.png", false), image("q232-src.png", true)])
                .as_deref(),
            Some("Qwen3.8-27B-Atlassian-Q8-mlx reads text only — the image q232-src.png was not sent")
        );
        assert_eq!(
            withheld_images_notice(
                "local-model",
                &[
                    image("a.png", true),
                    image("from read_image (image/png)", true)
                ]
            )
            .as_deref(),
            Some(
                "local-model reads text only — the images a.png, from read_image (image/png) \
                 were not sent"
            )
        );
        assert_eq!(
            withheld_image_placeholder(&path_image_label("/Users/me/shots/q232-src.png")),
            "[image q232-src.png not sent: this model reads text only]"
        );
    }
    use tempfile;

    #[test]
    fn test_detect_image_path() {
        // Create a temporary PNG file with valid PNG magic numbers
        let temp_dir = tempfile::tempdir().unwrap();
        let png_path = temp_dir.path().join("test.png");
        let png_data = [
            0x89, 0x50, 0x4E, 0x47, // PNG magic number
            0x0D, 0x0A, 0x1A, 0x0A, // PNG header
            0x00, 0x00, 0x00, 0x0D, // Rest of fake PNG data
        ];
        std::fs::write(&png_path, png_data).unwrap();
        let png_path_str = png_path.to_str().unwrap();

        // Create a fake PNG (wrong magic numbers)
        let fake_png_path = temp_dir.path().join("fake.png");
        std::fs::write(&fake_png_path, b"not a real png").unwrap();

        // Test with valid PNG file using absolute path
        let text = format!("Here is an image {}", png_path_str);
        assert_eq!(detect_image_path(&text).as_deref(), Some(png_path_str));

        // Test with non-image file that has .png extension
        let text = format!("Here is a fake image {}", fake_png_path.to_str().unwrap());
        assert_eq!(detect_image_path(&text).as_deref(), None);

        // Test with nonexistent file
        let text = "Here is a fake.png that doesn't exist";
        assert_eq!(detect_image_path(text).as_deref(), None);

        // Test with non-image file
        let text = "Here is a file.txt";
        assert_eq!(detect_image_path(text).as_deref(), None);

        // Test with relative path (should not match)
        let text = "Here is a relative/path/image.png";
        assert_eq!(detect_image_path(text).as_deref(), None);
    }

    #[test]
    fn test_detect_image_path_with_spaces() {
        // Absolute path containing spaces (macOS screenshot style).
        let temp_dir = tempfile::tempdir().unwrap();
        let png_path = temp_dir.path().join("Screen Shot 2026.png");
        let png_data = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        std::fs::write(&png_path, png_data).unwrap();
        let png_path_str = png_path.to_str().unwrap();

        let text = format!("please describe {} for me", png_path_str);
        assert_eq!(detect_image_path(&text).as_deref(), Some(png_path_str));

        // Case-insensitive extension also matches.
        let upper = temp_dir.path().join("Another Shot.PNG");
        std::fs::write(&upper, png_data).unwrap();
        let upper_str = upper.to_str().unwrap();
        let text = format!("see {}", upper_str);
        assert_eq!(detect_image_path(&text).as_deref(), Some(upper_str));

        // Quoted path with spaces: the closing quote terminates the candidate.
        let text = format!("describe \"{}\" please", png_path_str);
        assert_eq!(detect_image_path(&text).as_deref(), Some(png_path_str));
        let text = format!("describe '{}'", png_path_str);
        assert_eq!(detect_image_path(&text).as_deref(), Some(png_path_str));
        let text = format!("describe “{}” please", png_path_str);
        assert_eq!(detect_image_path(&text).as_deref(), Some(png_path_str));
        let text = format!("describe «{}» please", png_path_str);
        assert_eq!(detect_image_path(&text).as_deref(), Some(png_path_str));

        // A stray closing quote in prose must not act as a terminator for an
        // unquoted path.
        let text = format!("here {}\" trailing", png_path_str);
        assert_eq!(detect_image_path(&text).as_deref(), Some(png_path_str));

        // When a spaced filename contains an earlier image extension, prefer
        // the longer existing candidate over the embedded prefix.
        let edited = temp_dir.path().join("Screen Shot.png edited.jpg");
        std::fs::write(&edited, png_data).unwrap();
        let edited_str = edited.to_str().unwrap();
        let prefix = temp_dir.path().join("Screen Shot.png");
        std::fs::write(&prefix, png_data).unwrap();
        let text = format!("look at {}", edited_str);
        assert_eq!(detect_image_path(&text).as_deref(), Some(edited_str));

        // With multiple distinct images, the first referenced one wins even if
        // a later one has a longer path.
        let a = temp_dir.path().join("a.png");
        std::fs::write(&a, png_data).unwrap();
        let longer = temp_dir.path().join("much-longer.png");
        std::fs::write(&longer, png_data).unwrap();
        let text = format!(
            "compare {} with {}",
            a.to_str().unwrap(),
            longer.to_str().unwrap()
        );
        assert_eq!(
            detect_image_path(&text).as_deref(),
            Some(a.to_str().unwrap())
        );
    }

    #[test]
    fn test_detect_image_path_with_shell_escaped_metacharacters() {
        let temp_dir = tempfile::tempdir().unwrap();
        let png_data = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let png_path = temp_dir
            .path()
            .join("Bob's Project (v2) & $draft [final].png");
        std::fs::write(&png_path, png_data).unwrap();
        let png_path_str = png_path.to_str().unwrap();

        let escaped_path = png_path_str
            .replace(' ', "\\ ")
            .replace('(', "\\(")
            .replace(')', "\\)")
            .replace('&', "\\&")
            .replace('$', "\\$")
            .replace('\'', "\\'")
            .replace('[', "\\[")
            .replace(']', "\\]");
        let text = format!("please describe {}", escaped_path);

        assert_eq!(detect_image_path(&text).as_deref(), Some(png_path_str));
    }

    #[test]
    fn test_detect_image_path_prefers_existing_literal_backslash_path() {
        let temp_dir = tempfile::tempdir().unwrap();
        let png_data = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let png_path = temp_dir.path().join("literal\\&name.png");
        std::fs::write(&png_path, png_data).unwrap();
        let png_path_str = png_path.to_str().unwrap();
        let text = format!("please describe {}", png_path_str);

        assert_eq!(detect_image_path(&text).as_deref(), Some(png_path_str));
    }

    #[test]
    fn test_detect_image_path_with_unicode_separators() {
        let temp_dir = tempfile::tempdir().unwrap();
        let png_data = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let png_path = temp_dir.path().join("photo.png");
        std::fs::write(&png_path, png_data).unwrap();
        let png_path_str = png_path.to_str().unwrap();

        assert_eq!(
            detect_image_path(&format!("{png_path_str}🙂")).as_deref(),
            Some(png_path_str)
        );
        assert_eq!(
            detect_image_path(&format!("{png_path_str}🇺🇸")).as_deref(),
            Some(png_path_str)
        );
        assert_eq!(
            detect_image_path(&format!("{png_path_str}⌚")).as_deref(),
            Some(png_path_str)
        );
        assert_eq!(
            detect_image_path(&format!("{png_path_str}⭐")).as_deref(),
            Some(png_path_str)
        );
        assert_eq!(
            detect_image_path(&format!("{png_path_str}… more text")).as_deref(),
            Some(png_path_str)
        );
        assert_eq!(
            detect_image_path(&format!("{png_path_str}\u{2014}more text")).as_deref(),
            Some(png_path_str)
        );

        assert_eq!(
            detect_image_path(&format!("{png_path_str}\u{200B}.backup")).as_deref(),
            None
        );
        assert_eq!(
            detect_image_path(&format!("{png_path_str}\u{0301}")).as_deref(),
            None
        );
        assert_eq!(
            detect_image_path(&format!("file:{png_path_str}")).as_deref(),
            None
        );
    }

    #[test]
    fn test_detect_image_path_ignores_urls_and_longer_extensions() {
        let temp_dir = tempfile::tempdir().unwrap();
        let png_data = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

        // A real image whose path is a suffix of a URL must not be extracted
        // from that URL via the `://` separator.
        let dir = temp_dir.path().to_str().unwrap().trim_start_matches('/');
        let png_path = temp_dir.path().join("photo.png");
        std::fs::write(&png_path, png_data).unwrap();
        let url = format!("https:/{}/photo.png", dir);
        assert_eq!(detect_image_path(&url).as_deref(), None);

        // A backup file sharing the image extension prefix must not be
        // truncated to the bare image path.
        let real = temp_dir.path().join("shot.png");
        std::fs::write(&real, png_data).unwrap();
        let backup = format!("{}.backup", real.to_str().unwrap());
        assert_eq!(detect_image_path(&backup).as_deref(), None);
    }

    #[test]
    fn test_detect_image_path_ignores_extension_flood() {
        // Many extension-like tokens but no real absolute path: must scan
        // cheaply (bounded) and find nothing.
        let text = "see foo.png and bar.jpg and baz.jpeg ".repeat(500);
        assert_eq!(detect_image_path(&text).as_deref(), None);
    }

    #[test]
    fn test_load_image_file() {
        // Create a temporary PNG file with valid PNG magic numbers
        let temp_dir = tempfile::tempdir().unwrap();
        let png_path = temp_dir.path().join("test.png");
        let png_data = [
            0x89, 0x50, 0x4E, 0x47, // PNG magic number
            0x0D, 0x0A, 0x1A, 0x0A, // PNG header
            0x00, 0x00, 0x00, 0x0D, // Rest of fake PNG data
        ];
        std::fs::write(&png_path, png_data).unwrap();
        let png_path_str = png_path.to_str().unwrap();

        // Create a fake PNG (wrong magic numbers)
        let fake_png_path = temp_dir.path().join("fake.png");
        std::fs::write(&fake_png_path, b"not a real png").unwrap();
        let fake_png_path_str = fake_png_path.to_str().unwrap();

        // Test loading valid PNG file
        let result = load_image_file(png_path_str);
        assert!(result.is_ok());
        let image = result.unwrap();
        assert_eq!(image.mime_type, "image/png");

        // Test loading fake PNG file
        let result = load_image_file(fake_png_path_str);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not a valid image"));

        // Test nonexistent file
        let result = load_image_file("nonexistent.png");
        assert!(result.is_err());

        // Create a GIF file with valid header bytes
        let gif_path = temp_dir.path().join("test.gif");
        // Minimal GIF89a header
        let gif_data = [0x47, 0x49, 0x46, 0x38, 0x39, 0x61];
        std::fs::write(&gif_path, gif_data).unwrap();
        let gif_path_str = gif_path.to_str().unwrap();

        // Test loading unsupported GIF format
        let result = load_image_file(gif_path_str);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unsupported image format"));
    }
}
