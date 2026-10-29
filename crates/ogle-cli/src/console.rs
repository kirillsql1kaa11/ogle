use std::os::windows::io::AsRawHandle;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Console::{
    AllocConsole, AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE,
    STD_OUTPUT_HANDLE,
};

pub fn attach() {
    unsafe {
        let valid = matches!(GetStdHandle(STD_OUTPUT_HANDLE), Ok(handle) if !handle.is_invalid());
        if valid {
            return;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS).is_err() {
            let _ = AllocConsole();
        }
        if let Ok(out) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
            let handle = HANDLE(out.as_raw_handle() as _);
            let _ = SetStdHandle(STD_OUTPUT_HANDLE, handle);
            let _ = SetStdHandle(STD_ERROR_HANDLE, handle);
            std::mem::forget(out);
        }
    }
}
