use crate::detect::Format;
use crate::preview::{OgleError, Preview, PreviewDetails, PreviewRequest};

pub fn render(request: &PreviewRequest, format: Format) -> Result<Preview, OgleError> {
    let image = image::open(&request.path)?;
    let (width, height) = (image.width(), image.height());
    let thumb = super::fit_within(&image.to_rgba8(), request.max_size);

    let source = request
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| request.path.display().to_string());

    Ok(Preview {
        image: thumb,
        details: PreviewDetails {
            format,
            source,
            original_size: Some((width, height)),
            notes: Vec::new(),
        },
    })
}
