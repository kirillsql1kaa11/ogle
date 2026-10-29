//! PDF-превью через Windows.Data.Pdf (WinRT): без внешних DLL и pdfium.

use std::path::Path;

use crate::detect::Format;
use crate::preview::{OgleError, Preview, PreviewDetails, PreviewRequest};

pub fn render(request: &PreviewRequest) -> Result<Preview, OgleError> {
    let path = request.path.clone();
    let bytes = std::thread::Builder::new()
        .name("ogle-pdf".to_string())
        .spawn(move || render_first_page(&path))
        .map_err(|error| OgleError::Pdf(error.to_string()))?
        .join()
        .map_err(|_| OgleError::Pdf("поток рендера PDF упал".to_string()))??;

    let image = image::load_from_memory(&bytes)?;
    let (original_width, original_height) = (image.width(), image.height());
    let thumb = super::fit_within(&image.to_rgba8(), request.max_size);
    let source = request
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| request.path.display().to_string());

    Ok(Preview {
        image: thumb,
        details: PreviewDetails {
            format: Format::Pdf,
            source,
            original_size: Some((original_width, original_height)),
            notes: Vec::new(),
        },
    })
}

fn render_first_page(path: &Path) -> Result<Vec<u8>, OgleError> {
    use windows::core::HSTRING;
    use windows::Data::Pdf::PdfDocument;
    use windows::Storage::StorageFile;
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

    struct ComGuard(bool);
    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    if hr.is_err() {
        return Err(OgleError::Pdf(format!("COM init: 0x{:08X}", hr.0)));
    }
    let _guard = ComGuard(true);

    {
        // WinRT StorageFile не принимает смешанные разделители: нормализуем в `\`.
        let native_path = path.as_os_str().to_string_lossy().replace('/', "\\");
        let wide_path = HSTRING::from(native_path.as_str());
        let file = StorageFile::GetFileFromPathAsync(&wide_path)
            .map_err(winrt)?
            .get()
            .map_err(winrt)?;
        let document = PdfDocument::LoadFromFileAsync(&file)
            .map_err(winrt)?
            .get()
            .map_err(winrt)?;
        if document.PageCount().map_err(winrt)? == 0 {
            return Err(OgleError::NoPreview("PDF без страниц".to_string()));
        }

        let page = document.GetPage(0).map_err(winrt)?;

        let stream = InMemoryRandomAccessStream::new().map_err(winrt)?;
        page.RenderToStreamAsync(&stream)
            .map_err(winrt)?
            .get()
            .map_err(winrt)?;

        let size = stream.Size().map_err(winrt)? as usize;
        stream.Seek(0).map_err(winrt)?;
        let input = stream.GetInputStreamAt(0).map_err(winrt)?;
        let reader = DataReader::CreateDataReader(&input).map_err(winrt)?;
        reader
            .LoadAsync(size as u32)
            .map_err(winrt)?
            .get()
            .map_err(winrt)?;
        let mut buffer = vec![0u8; size];
        reader.ReadBytes(&mut buffer).map_err(winrt)?;
        Ok(buffer)
    }
}

fn winrt(error: windows::core::Error) -> OgleError {
    OgleError::Pdf(format!(
        "{} (HRESULT 0x{:08X})",
        error.message(),
        error.code().0 as u32
    ))
}
