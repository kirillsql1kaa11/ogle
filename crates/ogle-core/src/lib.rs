//! Ogle core: определение форматов и рендер превью без извлечения файлов на диск.

pub mod detect;
pub mod preview;
pub mod providers;

pub use detect::{detect_path, sniff, Format};
pub use image::RgbaImage;
pub use preview::{render, OgleError, Preview, PreviewDetails, PreviewRequest};
pub use providers::archive::{list_entries, EntryInfo};
