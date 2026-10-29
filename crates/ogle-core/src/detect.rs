use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Формат файла, определённый по сигнатуре.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Format {
    Image,
    Psd,
    Pdf,
    Epub,
    Zip,
    Rar,
    SevenZip,
    Video,
    #[default]
    Unknown,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::Image => "изображение",
            Format::Psd => "Photoshop (PSD)",
            Format::Pdf => "PDF",
            Format::Epub => "EPUB",
            Format::Zip => "архив ZIP",
            Format::Rar => "архив RAR",
            Format::SevenZip => "архив 7z",
            Format::Video => "видео",
            Format::Unknown => "неизвестный",
        }
    }

    pub fn supports_preview(self) -> bool {
        matches!(self, Format::Image | Format::Zip | Format::Epub)
    }
}

/// Определяет формат по первым байтам файла.
pub fn sniff(bytes: &[u8]) -> Format {
    if bytes.len() >= 8 && bytes[..4] == [0x89, b'P', b'N', b'G'] {
        return Format::Image;
    }
    if bytes.len() >= 3 && bytes[..3] == [0xFF, 0xD8, 0xFF] {
        return Format::Image;
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Format::Image;
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Format::Image;
    }
    if bytes.starts_with(b"BM") {
        return Format::Image;
    }
    if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        return Format::Image;
    }
    if bytes.starts_with(b"8BPS") {
        return Format::Psd;
    }
    if bytes.starts_with(b"%PDF") {
        return Format::Pdf;
    }
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return Format::Zip;
    }
    if bytes.starts_with(b"Rar!\x1A\x07") {
        return Format::Rar;
    }
    if bytes.starts_with(b"7z\xBC\xAF\x27\x1C") {
        return Format::SevenZip;
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return Format::Video;
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"AVI " {
        return Format::Video;
    }
    if bytes.starts_with(b"\x1A\x45\xDF\xA3") {
        return Format::Video;
    }
    if bytes.starts_with(b"OggS") {
        return Format::Video;
    }
    Format::Unknown
}

pub fn read_head(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let mut buffer = vec![0u8; 64];
    let read = file.read(&mut buffer)?;
    buffer.truncate(read);
    Ok(buffer)
}

/// Определяет формат файла; ZIP с mimetype EPUB уточняется до [`Format::Epub`].
pub fn detect_path(path: &Path) -> std::io::Result<Format> {
    let format = sniff(&read_head(path)?);
    if format == Format::Zip && crate::providers::archive::is_epub(path) {
        return Ok(Format::Epub);
    }
    Ok(format)
}
