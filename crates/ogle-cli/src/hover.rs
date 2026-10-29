use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ogle_core::{detect, list_entries, render, Format, PreviewRequest, RgbaImage};
use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
    FrameRect, GetMonitorInfoW, MonitorFromPoint, SelectObject, SetBkMode, SetTextColor,
    StretchDIBits, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, DRAW_TEXT_FORMAT,
    DT_END_ELLIPSIS, DT_NOPREFIX, DT_SINGLELINE, HDC, HFONT, HGDIOBJ, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, PAINTSTRUCT, SRCCOPY, TRANSPARENT, BI_RGB,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, IServiceProvider, CLSCTX_ALL,
    COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::IOleWindow;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, UIA_DataItemControlTypeId, UIA_ListItemControlTypeId,
};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::Shell::{
    IFolderView, IShellBrowser, IShellItem, IShellWindows, SHGetKnownFolderPath, ShellWindows,
    FOLDERID_Desktop, KF_FLAG_DEFAULT, SID_STopLevelBrowser, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClassNameW,
    GetCursorPos, GetForegroundWindow, GetWindowLongPtrW, GetWindowRect,
    RegisterClassW, SetWindowLongPtrW, ShowWindow, TranslateMessage, PeekMessageW,
    GA_ROOT, GetAncestor, GWLP_USERDATA, HTTRANSPARENT, MSG, PM_REMOVE, SW_SHOWNOACTIVATE, WM_CREATE,
    WM_DESTROY, WM_ERASEBKGND, WM_NCHITTEST, WM_PAINT, WNDCLASSW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::app;

pub static HOVER_ENABLED: AtomicBool = AtomicBool::new(true);

const TICK: Duration = Duration::from_millis(120);
const SHOW_TICKS: u32 = 3;
const CARD_CLASS: PCWSTR = w!("OgleHoverCard");

const BG: (u8, u8, u8) = (32, 33, 36);
const BORDER: (u8, u8, u8) = (63, 65, 70);
const TEXT: (u8, u8, u8) = (240, 240, 240);
const MUTED: (u8, u8, u8) = (160, 163, 168);

pub fn start() {
    HOVER_ENABLED.store(app::hover_enabled(), Ordering::SeqCst);
    std::thread::spawn(watcher);
}

struct State {
    shown: Option<(PathBuf, HWND)>,
    candidate: Option<PathBuf>,
    ticks: u32,
}

impl State {
    fn hide(&mut self) {
        if let Some((_, hwnd)) = self.shown.take() {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
        }
        self.candidate = None;
        self.ticks = 0;
    }

    fn contains(&self, point: POINT) -> bool {
        let Some((_, hwnd)) = self.shown else {
            return false;
        };
        unsafe {
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                return false;
            }
            point.x >= rect.left
                && point.x < rect.right
                && point.y >= rect.top
                && point.y < rect.bottom
        }
    }
}

fn watcher() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let Ok(uia) = CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_ALL) else {
            return;
        };
        let instance = GetModuleHandleW(None).unwrap_or_default();
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(card_proc),
            hInstance: windows::Win32::Foundation::HINSTANCE(instance.0),
            lpszClassName: CARD_CLASS,
            ..Default::default()
        };
        let _ = RegisterClassW(&window_class);

        let mut state = State {
            shown: None,
            candidate: None,
            ticks: 0,
        };

        loop {
            pump_messages();
            if !HOVER_ENABLED.load(Ordering::Relaxed) {
                state.hide();
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }

            let mut point = POINT::default();
            let _ = GetCursorPos(&mut point);

            if state.contains(point) {
                std::thread::sleep(TICK);
                continue;
            }

            let foreground = GetForegroundWindow();
            let class_name = window_class_name(foreground);
            if !matches!(class_name.as_str(), "CabinetWClass" | "Progman" | "WorkerW") {
                state.hide();
                std::thread::sleep(TICK);
                continue;
            }

            match hovered_file(&uia, foreground, &class_name, point) {
                Some(path) => {
                    if state.shown.as_ref().map(|(shown, _)| shown.as_path() == path.as_path())
                        == Some(true)
                    {
                    } else if state.candidate.as_deref() == Some(path.as_path()) {
                        state.ticks += 1;
                        if state.ticks >= SHOW_TICKS {
                            state.hide();
                            let shown = show_card(&path, point);
                            if let Some(hwnd) = shown {
                                state.shown = Some((path.clone(), hwnd));
                                state.candidate = Some(path);
                            }
                        }
                    } else {
                        state.hide();
                        state.candidate = Some(path);
                        state.ticks = 0;
                    }
                }
                None => state.hide(),
            }

            std::thread::sleep(TICK);
        }
    }
}

unsafe fn pump_messages() {
    let mut message = MSG::default();
    while PeekMessageW(&mut message, HWND::default(), 0, 0, PM_REMOVE).0 > 0 {
        let _ = TranslateMessage(&message);
        DispatchMessageW(&message);
    }
}

fn window_class_name(hwnd: HWND) -> String {
    let mut buffer = [0u16; 128];
    let length = unsafe { GetClassNameW(hwnd, &mut buffer) };
    String::from_utf16_lossy(&buffer[..length.max(0) as usize])
}

fn hovered_file(uia: &IUIAutomation, window: HWND, class_name: &str, point: POINT) -> Option<PathBuf> {
    unsafe {
        let element = uia.ElementFromPoint(point).ok()?;
        let walker = uia.ControlViewWalker().ok()?;
        let mut current = element;
        let mut name: Option<String> = None;
        for _ in 0..6 {
            let control_type = current.CurrentControlType().ok()?;
            if control_type == UIA_ListItemControlTypeId || control_type == UIA_DataItemControlTypeId {
                let text = current.CurrentName().ok()?.to_string();
                if !text.trim().is_empty() {
                    name = Some(text.trim().to_string());
                }
                break;
            }
            current = walker.GetParentElement(&current).ok()?;
        }
        let name = name?;
        let folder = if class_name == "CabinetWClass" {
            folder_for_window(window)?
        } else {
            desktop_dir()?
        };
        resolve(&folder, &name)
    }
}

unsafe fn folder_for_window(window: HWND) -> Option<PathBuf> {
    let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL).ok()?;
    let count = windows.Count().ok()?;
    for index in 0..count {
        let Ok(dispatch) = windows.Item(&windows::core::VARIANT::from(index)) else {
            continue;
        };
        let Ok(provider) = dispatch.cast::<IServiceProvider>() else {
            continue;
        };
        let Ok(browser) = provider.QueryService::<IShellBrowser>(&SID_STopLevelBrowser) else {
            continue;
        };
        let Ok(ole) = browser.cast::<IOleWindow>() else {
            continue;
        };
        let Ok(hwnd) = ole.GetWindow() else {
            continue;
        };
        let root = GetAncestor(hwnd, GA_ROOT);
        let target_root = GetAncestor(window, GA_ROOT);
        if root != target_root || root.is_invalid() {
            continue;
        }
        let view = browser.QueryActiveShellView().ok()?.cast::<IFolderView>().ok()?;
        let folder = view.GetFolder::<IShellItem>().ok()?;
        let name = folder.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let path = name.to_string().ok()?;
        CoTaskMemFree(Some(name.0 as *const _));
        return Some(PathBuf::from(path));
    }
    None
}

unsafe fn desktop_dir() -> Option<PathBuf> {
    let path = SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, HANDLE::default()).ok()?;
    let text = path.to_string().ok()?;
    CoTaskMemFree(Some(path.0 as *const _));
    Some(PathBuf::from(text))
}

fn resolve(folder: &Path, name: &str) -> Option<PathBuf> {
    let direct = folder.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    let entries = std::fs::read_dir(folder).ok()?;
    for entry in entries.flatten().take(2000) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name.eq_ignore_ascii_case(name) {
            return Some(path);
        }
        if let Some(stem) = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()) {
            if stem.eq_ignore_ascii_case(name) {
                return Some(path);
            }
        }
    }
    None
}

struct Card {
    picture: Option<RgbaImage>,
    cover: Option<RgbaImage>,
    title: String,
    meta: String,
    rows: Vec<(String, String)>,
    scale: f32,
    width: i32,
    height: i32,
    picture_width: i32,
    picture_height: i32,
    cover_box: i32,
}

fn build_card(path: &Path) -> Option<Card> {
    let format = detect::detect_path(path).ok()?;
    if !format.supports_preview() {
        return None;
    }
    let title = path.file_name()?.to_string_lossy().into_owned();
    let size = std::fs::metadata(path).ok()?.len();
    let scale = scale_factor();

    let mut card = Card {
        picture: None,
        cover: None,
        title,
        meta: String::new(),
        rows: Vec::new(),
        scale,
        width: 0,
        height: 0,
        picture_width: 0,
        picture_height: 0,
        cover_box: 0,
    };

    match format {
        Format::Image => {
            let preview = render(&PreviewRequest::new(path).max_size(560)).ok()?;
            let (width, height) = preview.image.dimensions();
            card.meta = format!("{width}×{height} · {}", human(size));
            card.picture = Some(preview.image);
            return Some(card);
        }
        Format::Pdf => {
            card.meta = format!("PDF · {}", human(size));
            card.cover = render(&PreviewRequest::new(path).max_size(240))
                .ok()
                .map(|preview| preview.image);
        }
        _ => {
            let entries = list_entries(path).ok()?;
            let files: Vec<_> = entries.iter().filter(|entry| !entry.is_dir).collect();
            card.cover = render(&PreviewRequest::new(path).max_size(240))
                .ok()
                .map(|preview| preview.image);
            card.rows = files
                .iter()
                .take(8)
                .map(|entry| (entry.name.clone(), human(entry.size)))
                .collect();
            if files.len() > 8 {
                card.rows.push((format!("… ещё {}", files.len() - 8), String::new()));
            }
            card.meta = format!(
                "{} · {} · {}",
                format.label(),
                plural_files(files.len()),
                human(size)
            );
        }
    }
    Some(card)
}

unsafe fn show_card(path: &Path, point: POINT) -> Option<HWND> {
    let mut card = build_card(path)?;
    let scale = card.scale;
    let pad = px(scale, 14.0);

    if let Some(picture) = &card.picture {
        let (width, height) = picture.dimensions();
        let max_width = px(scale, 560.0);
        let max_height = px(scale, 430.0);
        let ratio = (max_width as f32 / width as f32)
            .min(max_height as f32 / height as f32)
            .min(1.0);
        card.picture_width = ((width as f32 * ratio).round() as i32).max(1);
        card.picture_height = ((height as f32 * ratio).round() as i32).max(1);
        card.width = card.picture_width + pad * 2;
        card.height = card.picture_height + pad * 2 + px(scale, 8.0) + px(scale, 20.0) + px(scale, 16.0);
    } else {
        card.cover_box = if card.cover.is_some() { px(scale, 200.0) } else { 0 };
        card.width = px(scale, 620.0);
        let rows_height = px(scale, 20.0) * card.rows.len() as i32 + px(scale, 12.0);
        let cover_height = card.cover_box;
        card.height = pad * 2
            + cover_height
                .max(px(scale, 20) + px(scale, 4) + px(scale, 16) + rows_height);
    }

    let mut origin = POINT {
        x: point.x + px(scale, 20),
        y: point.y + px(scale, 26),
    };
    if let Some(work) = work_area(point) {
        if origin.x + card.width > work.right - px(scale, 8) {
            origin.x = work.right - card.width - px(scale, 8);
        }
        if origin.x < work.left + px(scale, 8) {
            origin.x = work.left + px(scale, 8);
        }
        if origin.y + card.height > work.bottom - px(scale, 8) {
            origin.y = point.y - card.height - px(scale, 16);
        }
        if origin.y < work.top + px(scale, 8) {
            origin.y = work.top + px(scale, 8);
        }
    }

    let instance = GetModuleHandleW(None).ok()?;
    let card = Box::new(card);
    let raw = Box::into_raw(card);
    let hwnd = CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        CARD_CLASS,
        w!("Ogle"),
        WS_POPUP,
        origin.x,
        origin.y,
        (*raw).width,
        (*raw).height,
        HWND::default(),
        None,
        instance,
        Some(raw as *const _),
    )
    .ok()?;

    let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw as isize);
    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    let _ = windows::Win32::Graphics::Gdi::InvalidateRect(hwnd, None, false);
    let _ = windows::Win32::Graphics::Gdi::UpdateWindow(hwnd);
    Some(hwnd)
}

fn work_area(point: POINT) -> Option<RECT> {
    unsafe {
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
        if monitor.is_invalid() {
            return None;
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            Some(info.rcWork)
        } else {
            None
        }
    }
}

unsafe extern "system" fn card_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        WM_CREATE => {
            let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, lparam.0);
            LRESULT(0)
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut paint);
            paint_card(hwnd, hdc);
            let _ = EndPaint(hwnd, &paint);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_DESTROY => {
            let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Card;
            if !pointer.is_null() {
                let _ = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(pointer));
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

unsafe fn paint_card(hwnd: HWND, hdc: HDC) {
    let pointer = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Card;
    if pointer.is_null() {
        return;
    }
    let card = &*pointer;
    let scale = card.scale;
    let pad = px(scale, 14.0);
    let bounds = RECT {
        left: 0,
        top: 0,
        right: card.width,
        bottom: card.height,
    };

    let background = CreateSolidBrush(rgb(BG));
    FillRect(hdc, &bounds, background);
    let _ = DeleteObject(HGDIOBJ(background.0));
    let border = CreateSolidBrush(rgb(BORDER));
    FrameRect(hdc, &bounds, border);
    let _ = DeleteObject(HGDIOBJ(border.0));

    let title_font = create_font(scale, 16.0, 600);
    let body_font = create_font(scale, 12.0, 400);
    let meta_font = create_font(scale, 12.0, 400);

    if let Some(picture) = &card.picture {
        draw_bitmap(hdc, picture, pad, pad, card.picture_width, card.picture_height);
        let caption_y = pad + card.picture_height + px(scale, 8.0);
        draw_text(
            hdc,
            &card.title,
            RECT {
                left: pad,
                top: caption_y,
                right: card.width - pad,
                bottom: caption_y + px(scale, 20.0),
            },
            title_font,
            rgb(TEXT),
            DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        draw_text(
            hdc,
            &card.meta,
            RECT {
                left: pad,
                top: caption_y + px(scale, 20.0),
                right: card.width - pad,
                bottom: caption_y + px(scale, 20.0) + px(scale, 16.0),
            },
            meta_font,
            rgb(MUTED),
            DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
    } else {
        let mut text_left = pad;
        if let Some(cover) = &card.cover {
            draw_cover(hdc, cover, pad, pad, card.cover_box);
            text_left = pad + card.cover_box + px(scale, 14.0);
        }
        let text_right = card.width - pad;
        let title_y = pad;
        draw_text(
            hdc,
            &card.title,
            RECT {
                left: text_left,
                top: title_y,
                right: text_right,
                bottom: title_y + px(scale, 20.0),
            },
            title_font,
            rgb(TEXT),
            DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        let meta_y = title_y + px(scale, 20.0) + px(scale, 4.0);
        draw_text(
            hdc,
            &card.meta,
            RECT {
                left: text_left,
                top: meta_y,
                right: text_right,
                bottom: meta_y + px(scale, 16.0),
            },
            meta_font,
            rgb(MUTED),
            DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
        );

        if !card.rows.is_empty() {
            let rows_y = meta_y + px(scale, 16.0) + px(scale, 10.0);
            let separator = CreateSolidBrush(rgb(BORDER));
            let line = RECT {
                left: text_left,
                top: rows_y,
                right: text_right,
                bottom: rows_y + 1,
            };
            FillRect(hdc, &line, separator);
            let _ = DeleteObject(HGDIOBJ(separator.0));

            let mut y = rows_y + px(scale, 8.0);
            for (name, size) in &card.rows {
                draw_text(
                    hdc,
                    name,
                    RECT {
                        left: text_left,
                        top: y,
                        right: text_right - px(scale, 80.0),
                        bottom: y + px(scale, 20.0),
                    },
                    body_font,
                    rgb(TEXT),
                    DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX,
                );
                if !size.is_empty() {
                    draw_text(
                        hdc,
                        size,
                        RECT {
                            left: text_right - px(scale, 76.0),
                            top: y,
                            right: text_right,
                            bottom: y + px(scale, 20.0),
                        },
                        meta_font,
                        rgb(MUTED),
                        DT_SINGLELINE | DT_NOPREFIX,
                    );
                }
                y += px(scale, 20.0);
            }
        }
    }

    for font in [title_font, body_font, meta_font] {
        let _ = DeleteObject(HGDIOBJ(font.0));
    }
}

unsafe fn draw_bitmap(hdc: HDC, image: &RgbaImage, x: i32, y: i32, width: i32, height: i32) {
    let (source_width, source_height) = image.dimensions();
    let pixels = to_bgra(image);
    let mut info = BITMAPINFO::default();
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: source_width as i32,
        biHeight: -(source_height as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    StretchDIBits(
        hdc,
        x,
        y,
        width,
        height,
        0,
        0,
        source_width as i32,
        source_height as i32,
        Some(pixels.as_ptr() as *const _),
        &info,
        DIB_RGB_COLORS,
        SRCCOPY,
    );
}

unsafe fn draw_cover(hdc: HDC, image: &RgbaImage, x: i32, y: i32, box_size: i32) {
    let (width, height) = image.dimensions();
    let ratio = (box_size as f32 / width as f32).min(box_size as f32 / height as f32);
    let draw_width = ((width as f32 * ratio).round() as i32).max(1);
    let draw_height = ((height as f32 * ratio).round() as i32).max(1);
    let offset_x = x + (box_size - draw_width) / 2;
    let offset_y = y + (box_size - draw_height) / 2;
    draw_bitmap(hdc, image, offset_x, offset_y, draw_width, draw_height);
}

fn to_bgra(image: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity((image.width() * image.height() * 4) as usize);
    for pixel in image.pixels() {
        let alpha = pixel[3] as u32;
        let blend = |channel: u8, background: u8| -> u8 {
            ((channel as u32 * alpha + background as u32 * (255 - alpha) + 127) / 255) as u8
        };
        out.extend_from_slice(&[
            blend(pixel[2], BG.2),
            blend(pixel[1], BG.1),
            blend(pixel[0], BG.0),
            255,
        ]);
    }
    out
}

unsafe fn draw_text(
    hdc: HDC,
    text: &str,
    mut rect: RECT,
    font: HFONT,
    color: COLORREF,
    flags: DRAW_TEXT_FORMAT,
) {
    let previous = SelectObject(hdc, HGDIOBJ(font.0));
    SetTextColor(hdc, color);
    SetBkMode(hdc, TRANSPARENT);
    let mut buffer: Vec<u16> = text.encode_utf16().collect();
    DrawTextW(hdc, &mut buffer, &mut rect, flags);
    SelectObject(hdc, previous);
}

unsafe fn create_font(scale: f32, size: f32, weight: i32) -> HFONT {
    CreateFontW(
        -px(scale, size),
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        1,
        0,
        0,
        5,
        0,
        w!("Segoe UI"),
    )
}

fn px(scale: f32, value: impl Into<f64>) -> i32 {
    ((value.into() * scale as f64).round().max(1.0)) as i32
}

fn scale_factor() -> f32 {
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 {
        1.0
    } else {
        dpi as f32 / 96.0
    }
}

fn rgb(color: (u8, u8, u8)) -> COLORREF {
    COLORREF(color.0 as u32 | ((color.1 as u32) << 8) | ((color.2 as u32) << 16))
}

fn plural_files(count: usize) -> String {
    let tail = count % 100;
    let word = if (11..=14).contains(&tail) {
        "файлов"
    } else {
        match count % 10 {
            1 => "файл",
            2..=4 => "файла",
            _ => "файлов",
        }
    };
    format!("{count} {word}")
}

fn human(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let value = bytes as f64;
    if value < KB {
        format!("{bytes} Б")
    } else if value < KB * KB {
        format!("{:.1} КБ", value / KB)
    } else if value < KB * KB * KB {
        format!("{:.1} МБ", value / (KB * KB))
    } else {
        format!("{:.2} ГБ", value / (KB * KB * KB))
    }
}
