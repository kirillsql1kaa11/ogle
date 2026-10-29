#![cfg(windows)]
#![windows_subsystem = "windows"]

//! Ogle в системном трее: иконка, статус по наведению, быстрые действия.

use std::ffi::c_void;
use std::mem::size_of;
use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::OnceLock;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, BOOL, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, HANDLE, HINSTANCE, HWND, LPARAM,
    LRESULT, POINT, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, GetDC, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER,
    HGDIOBJ, BI_RGB, DIB_RGB_COLORS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, ShellExecuteW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO,
    NIIF_WARNING, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW, MessageBoxW, HICON,
    HMENU, ICONINFO, MB_ICONERROR, MB_OK, MF_SEPARATOR, MF_STRING, MSG, PostMessageW,
    PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SW_SHOWNORMAL, SetForegroundWindow,
    TPM_NONOTIFY, TPM_RETURNCMD, TrackPopupMenu, TranslateMessage, WM_APP, WM_DESTROY,
    WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
};

const WM_TRAY: u32 = WM_APP + 1;
const MENU_STATUS: usize = 1;
const MENU_FOLDER: usize = 2;
const MENU_README: usize = 3;
const MENU_EXIT: usize = 4;
const CLSID_OGLE: &str = "a3f6d8e2-5b4c-4f1a-9d7e-2c8b1f4e6a90";
const ICON_SIZE: i32 = 32;

static ICON_HANDLE: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();

fn main() {
    if let Err(error) = run() {
        unsafe {
            let text = HSTRING::from(error.as_str());
            MessageBoxW(
                HWND::default(),
                &text,
                w!("Ogle"),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

fn run() -> Result<(), String> {
    unsafe {
        let _mutex = CreateMutexW(None, BOOL(1), w!("Local\\OgleTray"))
            .map_err(|error| error.message())?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }

        let instance = GetModuleHandleW(None).map_err(|error| error.message())?;
        let class_name = w!("OgleTrayWindow");
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: HINSTANCE(instance.0),
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassW(&window_class) == 0 {
            return Err("не удалось зарегистрировать окно трея".to_string());
        }

        let hwnd = CreateWindowExW(
            Default::default(),
            class_name,
            w!("Ogle"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            HWND::default(),
            HMENU::default(),
            HINSTANCE(instance.0),
            None,
        )
        .map_err(|error| error.message())?;

        let icon = create_icon().ok_or_else(|| "не удалось создать иконку".to_string())?;
        ICON_HANDLE.store(icon.0 as isize, Ordering::SeqCst);
        add_tray(hwnd, icon.0 as isize);

        let mut message = MSG::default();
        while GetMessageW(&mut message, HWND::default(), 0, 0).0 > 0 {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        Ok(())
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let icon = ICON_HANDLE.load(Ordering::SeqCst);
    match message {
        WM_TRAY => {
            match lparam.0 as u32 {
                WM_LBUTTONUP => show_status(hwnd, icon),
                WM_RBUTTONUP => show_menu(hwnd, icon),
                _ => {}
            }
            LRESULT(0)
        }
        _ if message == taskbar_created() => {
            add_tray(hwnd, icon);
            LRESULT(0)
        }
        WM_DESTROY => {
            remove_tray(hwnd, icon);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

fn taskbar_created() -> u32 {
    *TASKBAR_CREATED.get_or_init(|| unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) })
}

fn fill_wide(destination: &mut [u16], text: &str) {
    let encoded: Vec<u16> = text
        .encode_utf16()
        .take(destination.len().saturating_sub(1))
        .collect();
    destination[..encoded.len()].copy_from_slice(&encoded);
    destination[encoded.len()] = 0;
}

fn tray_tip() -> &'static str {
    if is_registered() {
        "Ogle — превью включены: ZIP, CBZ, EPUB, 7z, RAR, PDF"
    } else {
        "Ogle — не установлен. ПКМ → меню"
    }
}

fn tray_data(hwnd: HWND, icon: isize) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: HICON(icon as *mut c_void),
        ..Default::default()
    };
    fill_wide(&mut data.szTip, tray_tip());
    data
}

fn add_tray(hwnd: HWND, icon: isize) {
    let data = tray_data(hwnd, icon);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &data);
    }
}

fn remove_tray(hwnd: HWND, icon: isize) {
    let data = tray_data(hwnd, icon);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

fn show_status(hwnd: HWND, icon: isize) {
    let registered = is_registered();
    let mut data = tray_data(hwnd, icon);
    data.uFlags = NIF_INFO;
    data.dwInfoFlags = if registered { NIIF_INFO } else { NIIF_WARNING };
    fill_wide(&mut data.szInfoTitle, "Ogle");
    fill_wide(
        &mut data.szInfo,
        if registered {
            "Превью включены: ZIP, CBZ, EPUB, 7z, RAR, PDF"
        } else {
            "Провайдер не зарегистрирован. Запустите register.ps1 из папки программы."
        },
    );
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &data);
    }
}

fn show_menu(hwnd: HWND, icon: isize) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let status_label = if is_registered() {
            "Статус: установлено"
        } else {
            "Статус: не установлено"
        };
        let status_text = HSTRING::from(status_label);
        let _ = AppendMenuW(menu, MF_STRING, MENU_STATUS, &status_text);
        let _ = AppendMenuW(menu, MF_STRING, MENU_FOLDER, w!("Открыть папку программы"));
        let _ = AppendMenuW(menu, MF_STRING, MENU_README, w!("Открыть README"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, MENU_EXIT, w!("Выход"));

        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let _ = SetForegroundWindow(hwnd);
        let command = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY,
            point.x,
            point.y,
            0,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
        let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));

        match command.0 as usize {
            MENU_STATUS => show_status(hwnd, icon),
            MENU_FOLDER => {
                if let Some(directory) = program_dir() {
                    open_path(&directory.to_string_lossy());
                }
            }
            MENU_README => open_readme(),
            MENU_EXIT => {
                remove_tray(hwnd, icon);
                let _ = DestroyWindow(hwnd);
            }
            _ => {}
        }
    }
}

fn is_registered() -> bool {
    let subkey = HSTRING::from(format!(r"Software\Classes\CLSID\{{{CLSID_OGLE}}}"));
    let mut key = HKEY::default();
    let result = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &subkey, 0, KEY_READ, &mut key) };
    if result == ERROR_SUCCESS {
        unsafe {
            let _ = RegCloseKey(key);
        }
        true
    } else {
        false
    }
}

fn program_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(|parent| parent.to_path_buf())
}

fn open_readme() {
    if let Some(directory) = program_dir() {
        let readme = directory.join("README.md");
        if readme.exists() {
            open_path(&readme.to_string_lossy());
            return;
        }
    }
    open_path("https://github.com/kirillsql1kaa11/ogle");
}

fn open_path(target: &str) {
    let file = HSTRING::from(target);
    unsafe {
        ShellExecuteW(
            HWND::default(),
            w!("open"),
            &file,
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// Иконка трея из встроенного PNG: RGBA -> 32bpp DIB + маска.
fn create_icon() -> Option<HICON> {
    let bytes = include_bytes!("../../../assets/ogle.png");
    let image = image::load_from_memory(bytes)
        .ok()?
        .resize_exact(
            ICON_SIZE as u32,
            ICON_SIZE as u32,
            image::imageops::FilterType::Lanczos3,
        )
        .to_rgba8();

    let mut info = BITMAPINFO::default();
    info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = ICON_SIZE;
    info.bmiHeader.biHeight = -ICON_SIZE;
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB.0;

    let mut bits: *mut c_void = std::ptr::null_mut();
    let hdc = unsafe { GetDC(HWND::default()) };
    let color_bitmap = unsafe {
        CreateDIBSection(
            hdc,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            HANDLE::default(),
            0,
        )
    }
    .ok()?;
    unsafe { ReleaseDC(HWND::default(), hdc) };

    let pixels = unsafe {
        std::slice::from_raw_parts_mut(
            bits as *mut u8,
            (ICON_SIZE * ICON_SIZE * 4) as usize,
        )
    };
    for (index, pixel) in image.pixels().enumerate() {
        let alpha = pixel[3] as u32;
        let base = index * 4;
        pixels[base] = ((pixel[2] as u32 * alpha + 127) / 255) as u8;
        pixels[base + 1] = ((pixel[1] as u32 * alpha + 127) / 255) as u8;
        pixels[base + 2] = ((pixel[0] as u32 * alpha + 127) / 255) as u8;
        pixels[base + 3] = alpha as u8;
    }

    let mask_bits = vec![0u8; ((ICON_SIZE * ICON_SIZE) / 8) as usize];
    let mask_bitmap = unsafe {
        CreateBitmap(
            ICON_SIZE,
            ICON_SIZE,
            1,
            1,
            Some(mask_bits.as_ptr() as *const c_void),
        )
    };

    let icon_info = ICONINFO {
        fIcon: BOOL(1),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask_bitmap,
        hbmColor: color_bitmap,
    };
    let icon = unsafe { CreateIconIndirect(&icon_info) }.ok();
    unsafe {
        let _ = DeleteObject(HGDIOBJ(color_bitmap.0));
        let _ = DeleteObject(HGDIOBJ(mask_bitmap.0));
    }
    icon
}
