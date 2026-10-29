use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::detect::Format;
use crate::preview::{OgleError, Preview, PreviewDetails, PreviewRequest};

#[derive(Debug, Clone)]
pub struct EntryInfo {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
}

const COVER_HINTS: [&str; 6] = ["cover", "folder", "poster", "preview", "thumb", "front"];
const IMAGE_EXTS: [&str; 8] = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"];
const MAX_CANDIDATES: usize = 12;
const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;

/// EPUB — это ZIP, внутри которого лежит mimetype = application/epub+zip.
pub fn is_epub(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return false;
    };
    let is_epub = match archive.by_name("mimetype") {
        Ok(mut entry) => {
            let mut buffer = Vec::new();
            let _ = entry.by_ref().take(64).read_to_end(&mut buffer);
            buffer.starts_with(b"application/epub+zip")
        }
        Err(_) => false,
    };
    is_epub
}

pub fn list_entries(path: &Path) -> Result<Vec<EntryInfo>, OgleError> {
    let file = File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut entries = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        entries.push(EntryInfo {
            name: entry.name().to_string(),
            size: entry.size(),
            is_dir: entry.is_dir(),
        });
    }
    Ok(entries)
}

fn file_name_lower(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

pub fn render(request: &PreviewRequest) -> Result<Preview, OgleError> {
    let epub = is_epub(&request.path);
    let file = File::open(&request.path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let total_entries = archive.len();

    let mut candidates: Vec<(usize, String, u64)> = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let lower = name.to_ascii_lowercase();
        let leaf = file_name_lower(&lower);
        if lower.starts_with("__macosx/") || leaf.starts_with("._") {
            continue;
        }
        let is_image = leaf.rsplit('.').next().map(|ext| IMAGE_EXTS.contains(&ext)).unwrap_or(false);
        if !is_image {
            continue;
        }
        let hinted = COVER_HINTS.iter().any(|hint| leaf.contains(hint));
        let score = u64::from(hinted) * 4_000_000 + entry.size().min(1_999_999);
        candidates.push((index, name, score));
    }

    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.2));
    candidates.truncate(MAX_CANDIDATES);

    for (index, name, _) in candidates {
        let mut entry = archive.by_index(index)?;
        if entry.size() > MAX_ENTRY_BYTES {
            continue;
        }
        let mut buffer = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buffer)?;
        let Ok(image) = image::load_from_memory(&buffer) else {
            continue;
        };
        let (width, height) = (image.width(), image.height());
        let thumb = super::fit_within(&image.to_rgba8(), request.max_size);
        let format = if epub { Format::Epub } else { Format::Zip };
        return Ok(Preview {
            image: thumb,
            details: PreviewDetails {
                format,
                source: name,
                original_size: Some((width, height)),
                notes: vec![format!("записей в архиве: {total_entries}")],
            },
        });
    }

    Err(OgleError::NoPreview(if epub {
        "в EPUB не найдено изображение обложки".to_string()
    } else {
        "в архиве нет изображений".to_string()
    }))
}
