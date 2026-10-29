# Ogle

Превью содержимого файлов прямо в Проводнике Windows — без распаковки и открытия. Плюс иконка в трее со статусом.

Ядро на Rust, интеграция — нативная COM-обёртка `IThumbnailProvider`, PDF рисуется системным `Windows.Data.Pdf` (без pdfium и внешних DLL).

## Возможности

| Формат | Превью | Как |
|---|---|---|
| ZIP, CBZ | да | крупнейшее/cover-изображение из архива |
| EPUB | да | обложка (cover/folder/poster) |
| 7z | да | sevenz-rust; solid-архив читается одним проходом (лимит 1 ГБ) |
| RAR / многотомные RAR | да | vendored unrar (C++), RAR3 + RAR5 |
| PDF | да | первая страница через `Windows.Data.Pdf` |
| PNG, JPEG, GIF, WebP, BMP, TIFF, ICO | да | декодирование и вписывание в рамку |
| PSD, видео, 3D | roadmap | — |

Ограничения: только 64-битная Windows; ZIP с методами сжатия кроме deflate/stored могут не открыться; подпись DLL отсутствует.

## Трей

`ogle-tray.exe` — иконка в системном трее:

- наведение показывает подсказку со статусом (установлено / не установлено);
- левый клик — уведомление со статусом;
- ПКМ — меню: статус, открыть папку программы, открыть README, выход.

Повторный запуск не создаёт вторую иконку (мьютекс `Local\OgleTray`). Иконка восстанавливается после перезапуска Explorer.

## Архитектура

```
crates/
  ogle-core/   детект форматов + провайдеры превью (zip/epub, 7z, rar, pdf, raster)
  ogle-cli/    ogle info / thumb / batch — отладка без Проводника
  ogle-shell/  cdylib: IThumbnailProvider + IClassFactory, CLSID Ogle
  ogle-tray/   Win32-трей: иконка, tooltip, меню
assets/
  ogle.png     иконка трея (встраивается в exe)
scripts/
  register.ps1 регистрация в HKCU для .zip, .cbz, .epub, .7z, .rar, .pdf
```

Поток данных: Проводник создаёт COM-объект по CLSID → `IInitializeWithFile::Initialize(path)` → `GetThumbnail(cx)` → `ogle-core::render` → `HBITMAP` (32bpp, premultiplied alpha). PDF рендерится в отдельном MTA-потоке, чтобы не блокировать STA Проводника.

## Сборка

```powershell
cargo build --release
```

Артефакты кладутся в `C:\Users\<user>\AppData\Local\ogle-target\release\` — путь задан в `.cargo/config.toml`, чтобы OneDrive не синхронизировал `target/`. Поменяйте его на свой при переносе проекта.

Результат:

- `ogle.exe` — CLI
- `ogle_shell.dll` — COM-провайдер
- `ogle-tray.exe` — трей

## CLI

```powershell
ogle info C:\path\photos.zip     # формат, размер, содержимое архива
ogle thumb C:\path\photos.zip   # PNG рядом с файлом (*.ogle.png)
ogle thumb book.pdf -s 256 -o cover.png
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

Восемь тестов: сигнатуры форматов, обложка из ZIP, EPUB-детект и рендер, сохранение пропорций, архив без картинок, 7z, PDF (WinRT), аккуратная ошибка на битом RAR.

## Roadmap

- PSD-композит, видео-кадры (Media Foundation), 3D-превью
- Кэш превью и настройка размера в реестре
- Превью-панель (`IPreviewHandler`) в дополнение к миниатюрам
- Установщик (MSI) и подпись DLL

## Лицензия

MIT
