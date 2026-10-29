use std::path::PathBuf;

use crate::detect::{self, Format};
use crate::providers;

/// Запрос на построение превью.
#[derive(Debug, Clone)]
pub struct PreviewRequest {
    pub path: PathBuf,
    pub max_size: u32,
}

impl PreviewRequest {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            max_size: 512,
        }
    }

    pub fn max_size(mut self, pixels: u32) -> Self {
        self.max_size = pixels.clamp(16, 4096);
        self
    }
}

/// Откуда взялась картинка и что о ней известно.
#[derive(Debug, Clone, Default)]
pub struct PreviewDetails {
    pub format: Format,
    pub source: String,
    pub original_size: Option<(u32, u32)>,
    pub notes: Vec<String>,
}

#[derive(Debug)]
pub struct Preview {
    pub image: crate::RgbaImage,
    pub details: PreviewDetails,
}

#[derive(Debug, thiserror::Error)]
pub enum OgleError {
    #[error("не удалось прочитать файл: {0}")]
    Io(#[from] std::io::Error),
    #[error("ошибка архива: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("ошибка изображения: {0}")]
    Image(#[from] image::ImageError),
    #[error("формат пока не поддерживается: {0}")]
    Unsupported(String),
    #[error("ошибка архива: {0}")]
    Archive(String),
    #[error("ошибка PDF: {0}")]
    Pdf(String),
    #[error("превью недоступно: {0}")]
    NoPreview(String),
}

/// Главная точка входа: определить формат и отрисовать превью.
pub fn render(request: &PreviewRequest) -> Result<Preview, OgleError> {
    let format = detect::detect_path(&request.path)?;
    providers::render(request, format)
}
