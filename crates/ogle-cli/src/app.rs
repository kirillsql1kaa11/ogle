use std::ffi::OsStr;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_DWORD,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, MB_ICONERROR, MB_ICONINFORMATION, MB_OK,
};

use crate::tray;

pub const CLSID: &str = "{a3f6d8e2-5b4c-4f1a-9d7e-2c8b1f4e6a90}";
pub const THUMB_HANDLER: &str = "{E357FCCD-A995-4576-B01F-234630154E96}";
pub const EXTENSIONS: [&str; 6] = [".zip", ".cbz", ".epub", ".7z", ".rar", ".pdf"];

const DLL_NAME: &str = "ogle_shell.dll";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "Ogle";

#[cfg(ogle_embedded_shell)]
const EMBEDDED_DLL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ogle_shell.dll"));
#[cfg(not(ogle_embedded_shell))]
const EMBEDDED_DLL: &[u8] = &[];

pub fn launch() -> Result<(), String> {
    install()?;
    crate::hover::start();
    tray::run()
}

pub fn install() -> Result<PathBuf, String> {
    let dll = prepare_dll()?;
    register(&dll)?;
    notify_shell();
    Ok(dll)
}

pub fn uninstall() -> Result<(), String> {
    for extension in EXTENSIONS {
        delete_tree(&format!(
            r"Software\Classes\{extension}\ShellEx\{THUMB_HANDLER}"
        ));
    }
    delete_tree(&format!(r"Software\Classes\CLSID\{CLSID}"));
    delete_value(RUN_KEY, RUN_VALUE);
    notify_shell();
    Ok(())
}

pub fn is_registered() -> bool {
    unsafe {
        let mut key = HKEY::default();
        let path = wide(&format!(r"Software\Classes\CLSID\{CLSID}\InprocServer32"));
        if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()), 0, KEY_READ, &mut key)
            != ERROR_SUCCESS
        {
            return false;
        }
        let mut kind = REG_SZ;
        let mut size = 0u32;
        let status = RegQueryValueExW(
            key,
            PCWSTR::null(),
            None,
            Some(&mut kind),
            None,
            Some(&mut size),
        );
        let _ = RegCloseKey(key);
        status == ERROR_SUCCESS && size > 2
    }
}

pub fn autostart_enabled() -> bool {
    unsafe {
        let mut key = HKEY::default();
        let path = wide(RUN_KEY);
        if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(path.as_ptr()), 0, KEY_READ, &mut key)
            != ERROR_SUCCESS
        {
            return false;
        }
        let name = wide(RUN_VALUE);
        let mut kind = REG_SZ;
        let mut size = 0u32;
        let status = RegQueryValueExW(
            key,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            None,
            Some(&mut size),
        );
        let _ = RegCloseKey(key);
        status == ERROR_SUCCESS
    }
}

pub fn set_autostart(enabled: bool) -> Result<(), String> {
    if enabled {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        set_string(RUN_KEY, Some(RUN_VALUE), &format!("\"{}\"", exe.display()))
    } else {
        delete_value(RUN_KEY, RUN_VALUE);
        Ok(())
    }
}

pub fn hover_enabled() -> bool {
    read_dword(r"Software\Ogle", "HoverPreview").unwrap_or(1) != 0
}

pub fn set_hover(enabled: bool) {
    let _ = set_dword(r"Software\Ogle", "HoverPreview", u32::from(enabled));
}

fn read_dword(path: &str, name: &str) -> Option<u32> {
    unsafe {
        let path_wide = wide(path);
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(path_wide.as_ptr()), 0, KEY_READ, &mut key)
            != ERROR_SUCCESS
        {
            return None;
        }
        let name_wide = wide(name);
        let mut kind = REG_DWORD;
        let mut value = 0u32;
        let mut size = 4u32;
        let status = RegQueryValueExW(
            key,
            PCWSTR(name_wide.as_ptr()),
            None,
            Some(&mut kind),
            Some(&mut value as *mut u32 as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);
        if status == ERROR_SUCCESS && kind == REG_DWORD {
            Some(value)
        } else {
            None
        }
    }
}

pub fn program_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

pub fn alert_info(title: &str, text: &str) {
    alert(title, text, MB_OK | MB_ICONINFORMATION);
}

pub fn alert_error(title: &str, text: &str) {
    alert(title, text, MB_OK | MB_ICONERROR);
}

fn alert(title: &str, text: &str, flags: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE) {
    let title = HSTRING::from(title);
    let text = HSTRING::from(text);
    unsafe {
        MessageBoxW(HWND::default(), &text, &title, flags);
    }
}

fn prepare_dll() -> Result<PathBuf, String> {
    let directory = local_dir()?;
    fs::create_dir_all(&directory)
        .map_err(|error| format!("не удалось создать {}: {error}", directory.display()))?;

    let source = if !EMBEDDED_DLL.is_empty() {
        EMBEDDED_DLL.to_vec()
    } else if let Some(adjacent) = program_dir()
        .map(|dir| dir.join(DLL_NAME))
        .filter(|path| path.is_file())
    {
        fs::read(&adjacent)
            .map_err(|error| format!("не удалось прочитать {}: {error}", adjacent.display()))?
    } else {
        Vec::new()
    };

    let target = if source.is_empty() {
        find_installed(&directory).ok_or_else(|| {
            "в ogle.exe нет встроенной ogle_shell.dll и рядом с exe её нет — пересоберите с OGLE_EMBED_SHELL=1"
                .to_string()
        })?
    } else {
        directory.join(format!("ogle_shell_{:08x}.dll", checksum(&source)))
    };

    if !source.is_empty() && !same_file(&target, &source) {
        fs::write(&target, &source).map_err(|error| {
            format!(
                "не удалось записать {}: {error}. Закройте Проводник и повторите.",
                target.display()
            )
        })?;
    }

    cleanup(&directory, &target);
    Ok(target)
}

fn find_installed(directory: &Path) -> Option<PathBuf> {
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in fs::read_dir(directory).ok()? {
        let path = entry.ok()?.path();
        let name = path.file_name()?.to_string_lossy().to_lowercase();
        if !name.starts_with("ogle_shell_") || path.extension().is_none() {
            continue;
        }
        let modified = fs::metadata(&path).and_then(|meta| meta.modified()).ok()?;
        if newest.as_ref().map_or(true, |(time, _)| modified > *time) {
            newest = Some((modified, path));
        }
    }
    newest.map(|(_, path)| path)
}

fn cleanup(directory: &Path, keep: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == keep {
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        if name.starts_with("ogle_shell") && name.ends_with(".dll") {
            let _ = fs::remove_file(path);
        }
    }
}

fn same_file(path: &Path, bytes: &[u8]) -> bool {
    match fs::metadata(path) {
        Ok(metadata) if metadata.len() == bytes.len() as u64 => {
            fs::read(path).map(|current| current == bytes).unwrap_or(false)
        }
        _ => false,
    }
}

fn checksum(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for byte in bytes {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

fn local_dir() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("Ogle"))
        .ok_or_else(|| "не найдена переменная LOCALAPPDATA".to_string())
}

fn register(dll: &Path) -> Result<(), String> {
    let clsid_key = format!(r"Software\Classes\CLSID\{CLSID}");
    set_string(&clsid_key, None, "Ogle thumbnail provider")?;
    set_dword(&clsid_key, "DisableProcessIsolation", 1)?;
    let inproc_key = format!(r"{clsid_key}\InprocServer32");
    set_string(&inproc_key, None, &dll.to_string_lossy())?;
    set_string(&inproc_key, Some("ThreadingModel"), "Apartment")?;

    for extension in EXTENSIONS {
        let key = format!(r"Software\Classes\{extension}\ShellEx\{THUMB_HANDLER}");
        set_string(&key, None, CLSID)?;
    }
    Ok(())
}

fn set_dword(path: &str, name: &str, value: u32) -> Result<(), String> {
    unsafe {
        let path_wide = wide(path);
        let mut key = HKEY::default();
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path_wide.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        );
        if status != ERROR_SUCCESS {
            return Err(format!(
                "реестр: не удалось открыть {path} (код {})",
                status.0
            ));
        }

        let name_wide = wide(name);
        let bytes = value.to_ne_bytes();
        let status = RegSetValueExW(key, PCWSTR(name_wide.as_ptr()), 0, REG_DWORD, Some(&bytes));
        let _ = RegCloseKey(key);
        if status != ERROR_SUCCESS {
            return Err(format!(
                "реестр: не удалось записать {path} (код {})",
                status.0
            ));
        }
        Ok(())
    }
}

fn set_string(path: &str, name: Option<&str>, value: &str) -> Result<(), String> {
    unsafe {
        let path_wide = wide(path);
        let mut key = HKEY::default();
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path_wide.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        );
        if status != ERROR_SUCCESS {
            return Err(format!(
                "реестр: не удалось открыть {path} (код {})",
                status.0
            ));
        }

        let data: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let name_wide = name.map(wide);
        let name_ptr = name_wide
            .as_ref()
            .map_or(PCWSTR::null(), |wide_name| PCWSTR(wide_name.as_ptr()));
        let bytes = std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2);
        let status = RegSetValueExW(key, name_ptr, 0, REG_SZ, Some(bytes));
        let _ = RegCloseKey(key);
        if status != ERROR_SUCCESS {
            return Err(format!(
                "реестр: не удалось записать {path} (код {})",
                status.0
            ));
        }
        Ok(())
    }
}

fn delete_tree(path: &str) {
    unsafe {
        let path_wide = wide(path);
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(path_wide.as_ptr()));
    }
}

fn delete_value(path: &str, name: &str) {
    unsafe {
        let path_wide = wide(path);
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(path_wide.as_ptr()), 0, KEY_WRITE, &mut key)
            == ERROR_SUCCESS
        {
            let name_wide = wide(name);
            let _ = RegDeleteValueW(key, PCWSTR(name_wide.as_ptr()));
            let _ = RegCloseKey(key);
        }
    }
}

fn notify_shell() {
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}

fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
