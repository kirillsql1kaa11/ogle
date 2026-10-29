#![cfg(windows)]

//! In-proc COM-сервер: IThumbnailProvider для Проводника.
//! Регистрация — scripts/register.ps1 (HKCU, без администратора).

use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::OnceLock;

use ogle_core::{render, PreviewRequest};
use windows::core::{implement, Error, Interface, Result as WinResult, GUID, HRESULT, IUnknown, PCWSTR};
use windows::Win32::Foundation::{
    BOOL, CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_FAIL, E_POINTER, HANDLE, HWND, S_FALSE,
    S_OK,
};
use windows::Win32::Graphics::Gdi::{
    CreateDIBSection, GetDC, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER, HBITMAP, BI_RGB,
    DIB_RGB_COLORS,
};
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
use windows::Win32::UI::Shell::PropertiesSystem::{IInitializeWithFile, IInitializeWithFile_Impl};
use windows::Win32::UI::Shell::{IThumbnailProvider, IThumbnailProvider_Impl, WTSAT_ARGB, WTS_ALPHATYPE};

pub const CLSID_OGLE: GUID = GUID::from_u128(0xa3f6_d8e2_5b4c_4f1a_9d7e_2c8b_1f4e_6a90);

static LIVE_OBJECTS: AtomicIsize = AtomicIsize::new(0);

#[implement(IThumbnailProvider, IInitializeWithFile)]
struct ThumbnailProvider {
    path: OnceLock<PathBuf>,
}

impl ThumbnailProvider {
    fn new() -> Self {
        LIVE_OBJECTS.fetch_add(1, Ordering::SeqCst);
        Self {
            path: OnceLock::new(),
        }
    }
}

impl Drop for ThumbnailProvider {
    fn drop(&mut self) {
        LIVE_OBJECTS.fetch_sub(1, Ordering::SeqCst);
    }
}

impl IInitializeWithFile_Impl for ThumbnailProvider_Impl {
    fn Initialize(&self, pszfilepath: &PCWSTR, _grfmode: u32) -> WinResult<()> {
        let path = unsafe { pszfilepath.to_string() }.map_err(|_| Error::from(E_FAIL))?;
        self.path
            .set(PathBuf::from(path))
            .map_err(|_| Error::from(E_FAIL))?;
        Ok(())
    }
}

impl IThumbnailProvider_Impl for ThumbnailProvider_Impl {
    fn GetThumbnail(
        &self,
        cx: u32,
        phbmp: *mut HBITMAP,
        pdwalpha: *mut WTS_ALPHATYPE,
    ) -> WinResult<()> {
        if phbmp.is_null() || pdwalpha.is_null() {
            return Err(Error::from(E_POINTER));
        }
        let path = self.path.get().cloned().ok_or_else(|| Error::from(E_FAIL))?;
        let preview = render(&PreviewRequest::new(path).max_size(cx.clamp(64, 1024)))
            .map_err(|_| Error::from(E_FAIL))?;
        let bitmap = rgba_to_hbitmap(&preview.image)?;
        unsafe {
            *phbmp = bitmap;
            *pdwalpha = WTSAT_ARGB;
        }
        Ok(())
    }
}

#[implement(IClassFactory)]
struct ThumbnailFactory;

impl IClassFactory_Impl for ThumbnailFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Option<&IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut c_void,
    ) -> WinResult<()> {
        if punkouter.is_some() {
            return Err(Error::from(CLASS_E_NOAGGREGATION));
        }
        let unknown: IUnknown = ThumbnailProvider::new().into();
        unsafe { unknown.query(riid, ppvobject) }.ok()
    }

    fn LockServer(&self, _flock: BOOL) -> WinResult<()> {
        Ok(())
    }
}

/// # Safety
///
/// Вызывается COM-загрузчиком Windows; все указатели должны быть валидны по контракту COM.
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    if *rclsid != CLSID_OGLE {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    let factory: IClassFactory = ThumbnailFactory.into();
    let unknown: IUnknown = factory.cast().unwrap();
    let result = unknown.query(riid, ppv);
    if result.is_ok() {
        S_OK
    } else {
        result
    }
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if LIVE_OBJECTS.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

/// RGBA8 -> 32bpp DIB с premultiplied alpha (формат WTSAT_ARGB).
fn rgba_to_hbitmap(image: &ogle_core::RgbaImage) -> WinResult<HBITMAP> {
    let (width, height) = image.dimensions();

    let mut info = BITMAPINFO::default();
    info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    info.bmiHeader.biWidth = width as i32;
    info.bmiHeader.biHeight = -(height as i32);
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB.0;

    let mut bits: *mut c_void = std::ptr::null_mut();
    let hdc = unsafe { GetDC(HWND::default()) };
    let bitmap = unsafe {
        CreateDIBSection(
            hdc,
            &info,
            DIB_RGB_COLORS,
            &mut bits,
            HANDLE::default(),
            0,
        )
    };
    unsafe { ReleaseDC(HWND::default(), hdc) };
    let bitmap = bitmap?;

    let pixels = unsafe {
        std::slice::from_raw_parts_mut(bits as *mut u8, (width as usize) * (height as usize) * 4)
    };
    for (index, pixel) in image.pixels().enumerate() {
        let alpha = pixel[3] as u32;
        let base = index * 4;
        pixels[base] = ((pixel[2] as u32 * alpha + 127) / 255) as u8;
        pixels[base + 1] = ((pixel[1] as u32 * alpha + 127) / 255) as u8;
        pixels[base + 2] = ((pixel[0] as u32 * alpha + 127) / 255) as u8;
        pixels[base + 3] = alpha as u8;
    }

    Ok(bitmap)
}
