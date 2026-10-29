#![windows_subsystem = "windows"]

mod app;
mod console;
mod hover;
mod tray;

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use ogle_core::{detect, render, Format, PreviewRequest};

#[derive(Parser)]
#[command(name = "ogle", version, about = "Ogle — превью файлов без открытия")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Показать формат файла и доступность превью
    Info { path: PathBuf },
    /// Отрисовать превью в PNG
    Thumb {
        path: PathBuf,
        /// Максимальный размер превью в пикселях
        #[arg(short, long, default_value_t = 512)]
        size: u32,
        /// Куда сохранить PNG
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
    /// Сделать превью для всех поддерживаемых файлов в папке
    Batch {
        dir: PathBuf,
        #[arg(short, long, default_value_t = 512)]
        size: u32,
        #[arg(short, long, default_value = "previews")]
        out: PathBuf,
    },
}

fn main() -> ExitCode {
    unsafe {
        let _ = windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }

    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        return match app::launch() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                app::alert_error("Ogle — ошибка", &error);
                ExitCode::FAILURE
            }
        };
    }

    match args[0].as_str() {
        "--install" | "-i" => match app::install() {
            Ok(_) => {
                app::alert_info(
                    "Ogle",
                    "Превью включены: ZIP, CBZ, EPUB, 7z, RAR, PDF.\nЕсли миниатюры не появились — перезапустите Проводник.",
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                app::alert_error("Ogle — ошибка", &error);
                ExitCode::FAILURE
            }
        },
        "--uninstall" | "-u" => match app::uninstall() {
            Ok(()) => {
                app::alert_info("Ogle", "Превью отключены.");
                ExitCode::SUCCESS
            }
            Err(error) => {
                app::alert_error("Ogle — ошибка", &error);
                ExitCode::FAILURE
            }
        },
        _ => run_cli(),
    }
}

fn run_cli() -> ExitCode {
    console::attach();

    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let displayed = matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
                    | clap::error::ErrorKind::DisplayVersion
            );
            let _ = error.print();
            if displayed {
                return ExitCode::SUCCESS;
            }
            return ExitCode::from(2);
        }
    };

    let result = match cli.command {
        Command::Info { path } => info(&path),
        Command::Thumb { path, size, out } => thumb(&path, size, out),
        Command::Batch { dir, size, out } => batch(&dir, size, &out),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("ogle: {err}");
            ExitCode::FAILURE
        }
    }
}

fn info(path: &Path) -> Result<(), Box<dyn Error>> {
    let meta = fs::metadata(path)?;
    let format = detect::detect_path(path)?;

    println!("Файл:   {}", path.display());
    println!("Размер: {}", human(meta.len()));
    println!("Формат: {}", format.label());
    println!(
        "Превью: {}",
        if format.supports_preview() { "да" } else { "пока нет" }
    );

    if matches!(
        format,
        Format::Zip | Format::Epub | Format::SevenZip | Format::Rar
    ) {
        let entries = ogle_core::list_entries(path)?;
        let files: Vec<_> = entries.iter().filter(|entry| !entry.is_dir).collect();
        println!("Файлов внутри: {}", files.len());
        for entry in files.iter().take(15) {
            println!("  {:<60} {:>10}", entry.name, human(entry.size));
        }
        if files.len() > 15 {
            println!("  ...");
        }
    }
    Ok(())
}

fn thumb(path: &Path, size: u32, out: Option<PathBuf>) -> Result<(), Box<dyn Error>> {
    let preview = render(&PreviewRequest::new(path).max_size(size))?;
    let out = out.unwrap_or_else(|| {
        let stem = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "preview".to_string());
        path.with_file_name(format!("{stem}.ogle.png"))
    });

    preview.image.save(&out)?;
    let (width, height) = preview.image.dimensions();
    println!(
        "Превью: {} ({width}x{height}, источник: {})",
        out.display(),
        preview.details.source
    );
    Ok(())
}

fn batch(dir: &Path, size: u32, out: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(out)?;

    let mut rendered = 0u32;
    let mut skipped = 0u32;
    let mut failed = 0u32;

    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let Ok(format) = detect::detect_path(&path) else {
            skipped += 1;
            continue;
        };
        if !format.supports_preview() {
            skipped += 1;
            continue;
        }
        match render(&PreviewRequest::new(&path).max_size(size)) {
            Ok(preview) => {
                let stem = path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "preview".to_string());
                preview.image.save(out.join(format!("{stem}.png")))?;
                rendered += 1;
            }
            Err(_) => failed += 1,
        }
    }

    println!("Готово: {rendered}, пропущено: {skipped}, ошибок: {failed}");
    println!("Папка с превью: {}", out.display());
    Ok(())
}

fn human(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let value = bytes as f64;
    if value < KB {
        format!("{bytes} B")
    } else if value < KB * KB {
        format!("{:.1} KB", value / KB)
    } else if value < KB * KB * KB {
        format!("{:.1} MB", value / (KB * KB))
    } else {
        format!("{:.2} GB", value / (KB * KB * KB))
    }
}
