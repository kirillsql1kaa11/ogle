//! Общая логика архивных превью: сбор записей, выбор кандидатов, сборка результата.

pub mod rar;
pub mod sevenz;
pub mod zip;

use std::path::Path;

use crate::detect::Format;
use crate::preview::{OgleError, Preview, PreviewDetails, PreviewRequest};

#[derive(Debug, Clone)]
pub struct EntryInfo {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
}

pub(crate) const MAX_CANDIDATES: usize = 12;
pub(crate) const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
pub(crate) const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;

const COVER_HINTS: [&str; 6] = ["cover", "folder", "poster", "preview", "thumb", "front"];
const IMAGE_EXTS: [&str; 8] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"];

/// EPUB — это ZIP, внутри которого лежит mimetype = application/epub+zip.
pub fn is_epub(path: &Path) -> bool {
    zip::is_epub(path)
}

pub fn list_entries(path: &Path) -> Result<Vec<EntryInfo>, OgleError> {
    let format = crate::detect::detect_path(path)?;
    match format {
        Format::Zip | Format::Epub => zip::list(path),
        Format::SevenZip => sevenz::list(path),
        Format::Rar => rar::list(path),
        other => Err(OgleError::Unsupported(other.label().to_string())),
    }
}

/// Индексы записей, которые стоит пробовать как обложку, в порядке приоритета.
pub(crate) fn rank(entries: &[EntryInfo]) -> Vec<usize> {
    let mut scored: Vec<(usize, u64)> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        if entry.is_dir {
            continue;
        }
        let lower = entry.name.replace('\\', "/").to_ascii_lowercase();
        let leaf = lower.rsplit('/').next().unwrap_or("");
        if lower.starts_with("__macosx/") || leaf.starts_with("._") {
            continue;
        }
        let is_image = leaf
            .rsplit('.')
            .next()
            .map(|ext| IMAGE_EXTS.contains(&ext))
            .unwrap_or(false);
        if !is_image {
            continue;
        }
        let hinted = COVER_HINTS.iter().any(|hint| leaf.contains(hint));
        let score = u64::from(hinted) * 4_000_000 + entry.size.min(1_999_999);
        scored.push((index, score));
    }
    scored.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
    scored
        .into_iter()
        .take(MAX_CANDIDATES)
        .map(|(index, _)| index)
        .collect()
}

pub(crate) fn build_preview(
    format: Format,
    source: String,
    bytes: Vec<u8>,
    total_entries: usize,
    request: &PreviewRequest,
) -> Result<Preview, OgleError> {
    let image = image::load_from_memory(&bytes)?;
    let (width, height) = (image.width(), image.height());
    let thumb = super::fit_within(&image.to_rgba8(), request.max_size);
    Ok(Preview {
        image: thumb,
        details: PreviewDetails {
            format,
            source,
            original_size: Some((width, height)),
            notes: vec![format!("записей в архиве: {total_entries}")],
        },
    })
}

pub(crate) fn no_images(format: Format) -> OgleError {
    OgleError::NoPreview(match format {
        Format::Epub => "в EPUB не найдено изображение обложки".to_string(),
        Format::Rar => "в RAR-архиве нет изображений".to_string(),
        Format::SevenZip => "в 7z-архиве нет изображений".to_string(),
        _ => "в архиве нет изображений".to_string(),
    })
}
