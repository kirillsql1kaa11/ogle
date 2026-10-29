# Ogle

Превью содержимого файлов прямо в Проводнике Windows — без распаковки и открытия.

Ogle смотрит внутрь архива, книги или картинки и отдаёт Проводнику готовую миниатюру. Ядро написано на Rust, интеграция — нативная COM-обёртка `IThumbnailProvider`.

## Статус MVP

| Формат | Превью | Как |
|---|---|---|
| ZIP, CBZ | да | крупнейшее/cover-изображение из архива |
| EPUB | да | обложка (cover/folder/poster) |
| PNG, JPEG, GIF, WebP, BMP, TIFF, ICO | да | декодирование и вписывание в рамку |
| PSD, PDF, видео, 3D | roadmap | — |
| RAR, 7z | roadmap | — |

Ограничения MVP: только 64-битная Windows; ZIP с методами сжатия кроме deflate/stored могут не открыться; подпись DLL отсутствует.

## Архитектура

```
crates/
  ogle-core/   детект формата по сигнатуре + рендер превью + метаданные
  ogle-cli/    ogle info / thumb / batch — отладка без Проводника
  ogle-shell/  cdylib: IThumbnailProvider + IClassFactory, CLSID Ogle
scripts/
  register.ps1 регистрация в HKCU для .zip, .cbz, .epub (без админа)
```

Поток данных: Проводник создаёт COM-объект по CLSID → `IInitializeWithFile::Initialize(path)` → `GetThumbnail(cx)` → `ogle-core::render` → `HBITMAP` (32bpp, premultiplied alpha).

## Сборка

```powershell
cargo build --release
```

Артефакты кладутся в `C:\Users\<user>\AppData\Local\ogle-target\release\` — путь задан в `.cargo/config.toml`, чтобы OneDrive не синхронизировал `target/`. Поменяйте его на свой при переносе проекта.

Результат:

- `ogle.exe` — CLI
- `ogle_shell.dll` — COM-провайдер

## CLI

```powershell
ogle info C:\path\photos.zip     # формат, размер, содержимое архива
ogle thumb C:\path\photos.zip   # PNG рядом с файлом (*.ogle.png)
ogle thumb book.epub -s 256 -o cover.png
ogle batch C:\path\to\folder -s 512 -o previews
```

## Установка в Проводник

```powershell
powershell -ExecutionPolicy Bypass -File scripts\register.ps1
```

Скрипт прописывает CLSID и ShellEx-хендлеры в `HKCU\Software\Classes` (права администратора не нужны). После установки:

1. Перезапустите Explorer, чтобы он подхватил DLL.
2. Сбросьте кэш миниатюр: `ie4uinit.exe -show`.
3. Отключение: `powershell -File scripts\register.ps1 -Unregister`.

Если миниатюры не появились: проверьте, что DLL — release-сборка и 64-битная (Explorer на x64 грузит только x64-провайдеры), и что путь в `InprocServer32` указывает на существующий файл.

## Тесты

```powershell
cargo test
```

Пять тестов: сигнатуры форматов, обложка из ZIP, EPUB-детект и рендер, сохранение пропорций, архив без картинок.

## Roadmap

- PDF (pdfium), видео-кадры (Media Foundation), 3D-превью
- PSD: композит слоёв
- RAR/7z через `unrar`/`sevenz-rust`
- Кэш превью и настройка размера в реестре
- Превью-панель (`IPreviewHandler`) в дополнение к миниатюрам
- Установщик (MSI) и подпись DLL

## Лицензия

MIT
