use std::fs::File;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use ogle_core::{detect, render, Format, PreviewRequest};

fn workdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ogle-test-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn png_bytes(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([200, 30, 60, 255]));
    let mut bytes = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
        .unwrap();
    bytes
}

fn make_zip(dir: &Path, name: &str, files: &[(&str, &[u8])]) -> PathBuf {
    let path = dir.join(name);
    let mut writer = zip::ZipWriter::new(File::create(&path).unwrap());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored);
    for (file_name, data) in files {
        writer.start_file(*file_name, options).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap();
    path
}

#[test]
fn sniff_recognizes_common_signatures() {
    assert_eq!(detect::sniff(&png_bytes(2, 2)), Format::Image);
    assert_eq!(detect::sniff(b"%PDF-1.7"), Format::Pdf);
    assert_eq!(detect::sniff(b"8BPS\x00\x01"), Format::Psd);
    assert_eq!(detect::sniff(b"PK\x03\x04rest"), Format::Zip);
    assert_eq!(detect::sniff(b"Rar!\x1A\x07rest"), Format::Rar);
    assert_eq!(detect::sniff(b"7z\xBC\xAF\x27\x1Crest"), Format::SevenZip);
    assert_eq!(detect::sniff(b"hello"), Format::Unknown);
}

#[test]
fn renders_cover_from_zip() {
    let dir = workdir("zip");
    let cover = png_bytes(8, 4);
    let archive = make_zip(
        &dir,
        "photos.zip",
        &[("docs/readme.txt", b"hi"), ("pics/cover.png", &cover)],
    );

    let preview = render(&PreviewRequest::new(&archive).max_size(256)).unwrap();
    assert_eq!(preview.details.format, Format::Zip);
    assert_eq!(preview.details.source, "pics/cover.png");
    assert_eq!(preview.image.dimensions(), (8, 4));
    assert!(preview.details.notes[0].contains('2'));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn detects_and_renders_epub() {
    let dir = workdir("epub");
    let cover = png_bytes(10, 10);
    let book = make_zip(
        &dir,
        "book.epub",
        &[
            ("mimetype", b"application/epub+zip"),
            ("OEBPS/Images/cover.png", &cover),
        ],
    );

    assert_eq!(detect::detect_path(&book).unwrap(), Format::Epub);
    let preview = render(&PreviewRequest::new(&book).max_size(64)).unwrap();
    assert_eq!(preview.details.format, Format::Epub);
    assert_eq!(preview.image.dimensions(), (10, 10));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn raster_preview_keeps_aspect_ratio() {
    let dir = workdir("raster");
    let path = dir.join("photo.png");
    std::fs::write(&path, png_bytes(100, 50)).unwrap();

    let preview = render(&PreviewRequest::new(&path).max_size(32)).unwrap();
    assert_eq!(preview.details.format, Format::Image);
    assert_eq!(preview.image.dimensions(), (32, 16));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn zip_without_images_reports_no_preview() {
    let dir = workdir("empty");
    let archive = make_zip(&dir, "text.zip", &[("a.txt", b"just text")]);
    let result = render(&PreviewRequest::new(&archive));
    assert!(matches!(result, Err(ogle_core::OgleError::NoPreview(_))));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn renders_cover_from_7z() {
    let dir = workdir("sevenz");
    let archive = dir.join("photos.7z");
    let mut writer = sevenz_rust::SevenZWriter::create(&archive).unwrap();
    let mut entry = sevenz_rust::SevenZArchiveEntry::new();
    entry.name = "pics/cover.png".to_string();
    entry.has_stream = true;
    writer
        .push_archive_entry(entry, Some(Cursor::new(png_bytes(6, 3))))
        .unwrap();
    writer.finish().unwrap();

    assert_eq!(detect::detect_path(&archive).unwrap(), Format::SevenZip);
    let preview = render(&PreviewRequest::new(&archive).max_size(64)).unwrap();
    assert_eq!(preview.details.format, Format::SevenZip);
    assert_eq!(preview.details.source, "pics/cover.png");
    assert_eq!(preview.image.dimensions(), (6, 3));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn broken_rar_reports_error_gracefully() {
    let dir = workdir("rar");
    let path = dir.join("broken.rar");
    std::fs::write(&path, b"Rar!\x1A\x07\x00garbage").unwrap();

    assert_eq!(detect::detect_path(&path).unwrap(), Format::Rar);
    assert!(render(&PreviewRequest::new(&path)).is_err());

    let _ = std::fs::remove_dir_all(&dir);
}

fn minimal_pdf(dir: &Path) -> PathBuf {
    let content = "1 0 0 rg\n0 0 200 100 re\nf\n";
    let objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R /Resources << >> >>"
            .to_string(),
        format!("<< /Length {} >>\nstream\n{}endstream", content.len(), content),
    ];

    let mut pdf = Vec::new();
    pdf.extend_from_slice(b"%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{}\nendobj\n", index + 1, object).as_bytes());
    }
    let xref_at = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in &offsets {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );

    let path = dir.join("demo.pdf");
    std::fs::write(&path, pdf).unwrap();
    path
}

#[test]
fn renders_pdf_first_page() {
    let dir = workdir("pdf");
    let path = minimal_pdf(&dir);

    assert_eq!(detect::detect_path(&path).unwrap(), Format::Pdf);
    let preview = render(&PreviewRequest::new(&path).max_size(128)).unwrap();
    assert_eq!(preview.details.format, Format::Pdf);
    assert!(preview.image.width() > 0 && preview.image.height() > 0);

    let _ = std::fs::remove_dir_all(&dir);
}
