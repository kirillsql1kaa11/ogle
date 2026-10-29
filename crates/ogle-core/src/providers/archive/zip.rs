use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::detect::Format;
use crate::preview::{OgleError, Preview, PreviewRequest};

use super::{build_preview, no_images, rank, EntryInfo, MAX_ENTRY_BYTES};

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

fn collect(archive: &mut zip::ZipArchive<File>) -> Result<Vec<EntryInfo>, OgleError> {
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

pub fn list(path: &Path) -> Result<Vec<EntryInfo>, OgleError> {
    let file = File::open(path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    collect(&mut archive)
}

pub fn render(request: &PreviewRequest) -> Result<Preview, OgleError> {
    let file = File::open(&request.path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let entries = collect(&mut archive)?;
    let total = entries.len();
    let format = if is_epub(&request.path) {
        Format::Epub
    } else {
        Format::Zip
    };

    for index in rank(&entries) {
        if entries[index].size > MAX_ENTRY_BYTES {
            continue;
        }
        let mut entry = archive.by_index(index)?;
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        let name = entries[index].name.clone();
        if let Ok(preview) = build_preview(format, name, bytes, total, request) {
            return Ok(preview);
        }
    }

    Err(no_images(format))
}
