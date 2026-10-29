use std::path::Path;

use unrar::Archive;

use crate::detect::Format;
use crate::preview::{OgleError, Preview, PreviewRequest};

use super::{build_preview, no_images, rank, EntryInfo, MAX_ENTRY_BYTES};

fn map_err(error: unrar::error::UnrarError) -> OgleError {
    OgleError::Archive(format!("{error}"))
}

pub fn list(path: &Path) -> Result<Vec<EntryInfo>, OgleError> {
    let listing = Archive::new(path).open_for_listing().map_err(map_err)?;
    let mut entries = Vec::new();
    for entry in listing {
        let entry = entry.map_err(map_err)?;
        entries.push(EntryInfo {
            name: entry.filename.to_string_lossy().into_owned(),
            size: entry.unpacked_size,
            is_dir: !entry.is_file(),
        });
    }
    Ok(entries)
}

pub fn render(request: &PreviewRequest) -> Result<Preview, OgleError> {
    let entries = list(&request.path)?;
    let total = entries.len();
    let ranked = rank(&entries);
    if ranked.is_empty() {
        return Err(no_images(Format::Rar));
    }
    let wanted: Vec<String> = ranked
        .iter()
        .map(|&index| entries[index].name.clone())
        .collect();

    let mut found: Vec<(String, Vec<u8>)> = Vec::new();
    let mut archive = Archive::new(&request.path)
        .open_for_processing()
        .map_err(map_err)?;
    while found.len() < wanted.len() {
        let Some(header) = archive.read_header().map_err(map_err)? else {
            break;
        };
        let entry = header.entry();
        let name = entry.filename.to_string_lossy().into_owned();
        let is_wanted = wanted.iter().any(|candidate| candidate == &name);
        if is_wanted && entry.is_file() && entry.unpacked_size <= MAX_ENTRY_BYTES {
            let (bytes, rest) = header.read().map_err(map_err)?;
            found.push((name, bytes));
            archive = rest;
        } else {
            archive = header.skip().map_err(map_err)?;
        }
    }

    for name in wanted {
        if let Some(position) = found.iter().position(|(stored, _)| stored == &name) {
            let (_, bytes) = found.swap_remove(position);
            if let Ok(preview) = build_preview(Format::Rar, name, bytes, total, request) {
                return Ok(preview);
            }
        }
    }

    Err(no_images(Format::Rar))
}
