use std::path::Path;

use sevenz_rust::{Archive, Password, SevenZReader};

use crate::detect::Format;
use crate::preview::{OgleError, Preview, PreviewRequest};

use super::{build_preview, no_images, rank, EntryInfo, MAX_ENTRY_BYTES, MAX_TOTAL_BYTES};

fn map_err(error: sevenz_rust::Error) -> OgleError {
    OgleError::Archive(format!("{error}"))
}

pub fn list(path: &Path) -> Result<Vec<EntryInfo>, OgleError> {
    let archive = Archive::open(path).map_err(map_err)?;
    Ok(archive
        .files
        .iter()
        .map(|entry| EntryInfo {
            name: entry.name().to_string(),
            size: entry.size,
            is_dir: entry.is_directory,
        })
        .collect())
}

pub fn render(request: &PreviewRequest) -> Result<Preview, OgleError> {
    let entries = list(&request.path)?;
    let total = entries.len();
    let total_size: u64 = entries.iter().map(|entry| entry.size).sum();
    if total_size > MAX_TOTAL_BYTES {
        return Err(OgleError::NoPreview(
            "7z-архив слишком большой для быстрого превью".to_string(),
        ));
    }

    let ranked = rank(&entries);
    if ranked.is_empty() {
        return Err(no_images(Format::SevenZip));
    }
    let wanted: Vec<String> = ranked
        .iter()
        .map(|&index| entries[index].name.clone())
        .collect();

    let mut reader = SevenZReader::open(&request.path, Password::empty()).map_err(map_err)?;
    let mut found: Vec<(String, Vec<u8>)> = Vec::new();
    let wanted_names = wanted.clone();
    reader
        .for_each_entries(|entry, data| {
            if wanted_names.iter().any(|name| name == entry.name())
                && entry.size <= MAX_ENTRY_BYTES
            {
                let mut bytes = Vec::with_capacity(entry.size as usize);
                if data.read_to_end(&mut bytes).is_ok() {
                    found.push((entry.name().to_string(), bytes));
                }
            }
            Ok(true)
        })
        .map_err(map_err)?;

    for name in wanted {
        if let Some(position) = found.iter().position(|(stored, _)| stored == &name) {
            let (_, bytes) = found.swap_remove(position);
            if let Ok(preview) = build_preview(Format::SevenZip, name, bytes, total, request) {
                return Ok(preview);
            }
        }
    }

    Err(no_images(Format::SevenZip))
}
