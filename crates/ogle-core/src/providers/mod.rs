pub mod archive;
#[cfg(windows)]
pub mod pdf;
pub mod raster;

use crate::detect::Format;
use crate::preview::{OgleError, Preview, PreviewRequest};
use crate::RgbaImage;

pub fn render(request: &PreviewRequest, format: Format) -> Result<Preview, OgleError> {
    match format {
        Format::Image => raster::render(request, format),
        Format::Zip | Format::Epub => archive::zip::render(request),
        Format::SevenZip => archive::sevenz::render(request),
        Format::Rar => archive::rar::render(request),
        Format::Pdf => render_pdf(request),
        other => Err(OgleError::Unsupported(other.label().to_string())),
    }
}

#[cfg(windows)]
fn render_pdf(request: &PreviewRequest) -> Result<Preview, OgleError> {
    pdf::render(request)
}

#[cfg(not(windows))]
fn render_pdf(request: &PreviewRequest) -> Result<Preview, OgleError> {
    let _ = request;
    Err(OgleError::Unsupported("PDF (только Windows)".to_string()))
}

/// Вписать изображение в квадрат `max` с сохранением пропорций и без апскейла.
pub fn fit_within(image: &RgbaImage, max: u32) -> RgbaImage {
    let (width, height) = image.dimensions();
    let scale = (max as f32 / width.max(height) as f32).min(1.0);
    if scale >= 1.0 {
        return image.clone();
    }
    let target_width = ((width as f32 * scale).round() as u32).max(1);
    let target_height = ((height as f32 * scale).round() as u32).max(1);
    image::imageops::resize(
        image,
        target_width,
        target_height,
        image::imageops::FilterType::Triangle,
    )
}
