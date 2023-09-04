// Windows object wrappers with a documented safety model
//
// Many wrappers live in submodules to prevent accidental access to
// the interior; access must be through unsafe raw()

use std::result::Result;
use thiserror::Error;
use windows::{
    core,
    core::*,
    Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*},
};

use crate::Color;

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0}")]
    Windows(#[from] core::Error),

    #[error("Window has been destroyed")]
    Destroyed,

    #[error("Unsupported bitmap format")]
    UnsupportedBitmapFormat,
}

pub struct WideZString(Vec<u16>);

impl WideZString {
    // TODO: translate newlines
    pub fn new(s: &str) -> Self {
        Self(s.encode_utf16().chain(Some(0)).collect())
    }

    pub fn pzwstr(&self) -> PCWSTR {
        PCWSTR(self.0.as_ptr())
    }
}

impl From<&str> for WideZString {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

pub trait Raw<T> {
    /// # Safety
    ///
    /// * Implementers ensure that the handle is valid for the lifetime of Self
    /// * Implementers ensure that the handle is not null
    /// * Implementers ensure that all calls to raw() return the same handle
    /// * Callers must not cause the handle to be released or destroyed
    /// * Callers must not cause the handle to be used after Self is dropped
    unsafe fn raw(&self) -> T;
}

mod raw_hwnd {
    use super::*;
    pub struct RawHwnd(HWND);

    impl RawHwnd {
        /// # Safety
        ///
        /// * Caller must ensure that the handle is valid for the lifetime of Self
        /// * Caller must ensure that the handle is not null
        pub unsafe fn new(hdc: HWND) -> Self {
            Self(hdc)
        }
    }

    impl Raw<HWND> for RawHwnd {
        // Safety: see Raw::raw()
        unsafe fn raw(&self) -> HWND {
            self.0
        }
    }
}
pub use raw_hwnd::RawHwnd;

/// (x, y, w, h)
pub fn get_window_rect(hwnd: &impl Raw<HWND>) -> Result<(i32, i32, i32, i32), Error> {
    let mut rect = RECT::default();
    // Safety: raw() ensures hwnd is valid
    unsafe { GetWindowRect(hwnd.raw(), &mut rect)? }
    Ok((
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
    ))
}

/// (x, y, w, h)
pub fn get_client_rect(hwnd: &impl Raw<HWND>) -> Result<(i32, i32, i32, i32), Error> {
    let mut rect = RECT::default();
    // Safety: raw() ensures hwnd is valid
    unsafe { GetClientRect(hwnd.raw(), &mut rect)? }
    Ok((
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
    ))
}

mod window_dc {
    use super::*;
    pub struct WindowDC<'a, Hwnd: Raw<HWND>>(HDC, &'a Hwnd);

    impl<'a, Hwnd: Raw<HWND>> WindowDC<'a, Hwnd> {
        pub fn new(hwnd: &'a Hwnd) -> Result<Self, Error> {
            // Safety: Self holds a ref to Hwnd, ensuring its lifetime
            let hdc = unsafe { GetDC(hwnd.raw()) };
            if hdc.0 == 0 {
                Err(core::Error::from_win32())?
            }
            Ok(Self(hdc, hwnd))
        }
    }

    impl<'a, Hwnd: Raw<HWND>> Drop for WindowDC<'a, Hwnd> {
        fn drop(&mut self) {
            // Safety: self.1.raw() ensures HWND is valid and unchanged.
            //         We ensure HDC is valid and unchanged.
            unsafe {
                ReleaseDC(self.1.raw(), self.0);
            }
        }
    }

    impl<'a, Hwnd: Raw<HWND>> Raw<HDC> for WindowDC<'a, Hwnd> {
        // Safety: see Raw::raw()
        unsafe fn raw(&self) -> HDC {
            self.0
        }
    }
}
pub use window_dc::WindowDC;

mod paint_dc {
    use super::*;

    pub struct PaintDC<'a, Hwnd: Raw<HWND>>(HDC, &'a Hwnd);

    impl<'a, Hwnd: Raw<HWND>> PaintDC<'a, Hwnd> {
        pub fn new(hwnd: &'a Hwnd) -> Result<Self, Error> {
            // Safety: Self holds a ref to Hwnd, ensuring its lifetime
            let hdc = unsafe { BeginPaint(hwnd.raw(), &mut PAINTSTRUCT::default()) };
            if hdc.0 == 0 {
                Err(core::Error::from_win32())?
            }
            Ok(Self(hdc, hwnd))
        }
    }

    impl<'a, Hwnd: Raw<HWND>> Drop for PaintDC<'a, Hwnd> {
        fn drop(&mut self) {
            // Safety: HDC::raw() ensures HWND is valid and unchanged
            unsafe {
                EndPaint(self.1.raw(), &PAINTSTRUCT::default());
            }
        }
    }

    impl<'a, Hwnd: Raw<HWND>> Raw<HDC> for PaintDC<'a, Hwnd> {
        // Safety: see Raw::raw()
        unsafe fn raw(&self) -> HDC {
            self.0
        }
    }
}
pub use paint_dc::PaintDC;

mod memory_dc {
    use super::*;
    pub struct MemoryDc(HDC);

    impl MemoryDc {
        pub fn compatible<OtherDC: Raw<HDC>>(other_dc: &OtherDC) -> Result<Self, Error> {
            // Safety: other_dc.raw() ensures HDC is valid. It doesn't need to live as
            //         long as Self, so we don't need to hold a reference to it.
            let dc = unsafe { MemoryDc(CreateCompatibleDC(other_dc.raw())) };
            if dc.0 .0 == 0 {
                Err(core::Error::from_win32())?
            }
            Ok(dc)
        }
    }

    impl Drop for MemoryDc {
        fn drop(&mut self) {
            // Safety: we ensure HDC is valid.
            unsafe {
                DeleteDC(self.0);
            }
        }
    }

    impl Raw<HDC> for MemoryDc {
        // Safety: see Raw::raw()
        unsafe fn raw(&self) -> HDC {
            self.0
        }
    }
}
pub use memory_dc::MemoryDc;

mod borrowed_gdiobj {
    use super::*;
    pub struct BorrowedGdiobj<'a, Owner>(&'a Owner, HGDIOBJ);

    impl<'a, Owner> BorrowedGdiobj<'a, Owner> {
        /// # Safety
        ///
        /// * Caller must ensure that gdiobj is valid for 'a
        /// * Caller must ensure that gdiobj is not null
        pub unsafe fn new(owner: &'a Owner, gdiobj: HGDIOBJ) -> Self {
            Self(owner, gdiobj)
        }
    }

    impl<'a, Owner> Raw<HGDIOBJ> for BorrowedGdiobj<'a, Owner> {
        // Safety: see Raw::raw()
        unsafe fn raw(&self) -> HGDIOBJ {
            self.1
        }
    }
}
pub use borrowed_gdiobj::BorrowedGdiobj;

/// Soundness compatability: this function isn't compatible with any unsafe code
/// which is careless with [SelectObject] scoping.
pub fn select_object<'a, R, DC: Raw<HDC>, Obj: Raw<HGDIOBJ>, F: FnOnce() -> Result<R, Error>>(
    dc: &'a DC,
    obj: &'a Obj,
    f: F,
) -> Result<R, Error> {
    struct Restore(HDC, HGDIOBJ);
    impl Drop for Restore {
        fn drop(&mut self) {
            // Safety: Outer function ensures HDC and HGDIOBJ are valid.
            unsafe {
                SelectObject(self.0, self.1);
            }
        }
    }

    // Safety: *.raw() ensures HDC and HGDIOBJ are valid.
    //         Since we hold references to Raw<*>, they are not
    //         destroyed until this function returns.
    let old = unsafe { SelectObject(dc.raw(), obj.raw()) };
    if old.0 == 0 {
        Err(core::Error::from_win32())?
    }

    // Safety: We ensure that old is valid and unchanged.
    let _restore = unsafe { Restore(dc.raw(), old) };

    f()
}

mod hbitmap {
    use super::*;
    pub struct HBitmap(HBITMAP);

    impl HBitmap {
        pub fn compatible<DC: Raw<HDC>>(dc: &DC, width: i32, height: i32) -> Result<Self, Error> {
            // Safety: dc.raw() ensures HDC is valid. It doesn't need to live as
            //         long as Self, so we don't need to hold a reference to it.
            let bm = unsafe { HBitmap(CreateCompatibleBitmap(dc.raw(), width, height)) };
            if bm.0 .0 == 0 {
                Err(core::Error::from_win32())?
            }
            Ok(bm)
        }

        pub fn gdiobj(&self) -> BorrowedGdiobj<Self> {
            // Safety: we ensure HBITMAP/HGDIOBJ is valid for our lifetime.
            unsafe { BorrowedGdiobj::new(self, HGDIOBJ(self.0 .0)) }
        }
    }

    impl Drop for HBitmap {
        fn drop(&mut self) {
            // Safety: we ensure HBITMAP is valid.
            unsafe {
                DeleteObject(self.0);
            }
        }
    }

    impl Raw<HBITMAP> for HBitmap {
        // Safety: see Raw::raw()
        unsafe fn raw(&self) -> HBITMAP {
            self.0
        }
    }
}
pub use hbitmap::HBitmap;

mod hbrush {
    use super::*;

    pub struct HBrush(HBRUSH);

    impl HBrush {
        pub fn solid(color: Color) -> Result<Self, Error> {
            // Safety: we ensure HBRUSH is valid.
            let brush = unsafe {
                HBrush(CreateSolidBrush(COLORREF(
                    (color.0 as u32) | ((color.1 as u32) << 8) | ((color.2 as u32) << 16),
                )))
            };
            if brush.0 .0 == 0 {
                Err(core::Error::from_win32())?
            }
            Ok(brush)
        }
    }

    impl Drop for HBrush {
        fn drop(&mut self) {
            // Safety: we ensure HBRUSH is valid.
            unsafe {
                DeleteObject(self.0);
            }
        }
    }

    impl Raw<HBRUSH> for HBrush {
        // Safety: see Raw::raw()
        unsafe fn raw(&self) -> HBRUSH {
            self.0
        }
    }
}
pub use hbrush::*;

pub fn fill_rect<'a, DC: Raw<HDC>, Brush: Raw<HBRUSH>>(
    dc: &'a DC,
    brush: &'a Brush,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) {
    // Safety: dc.raw() and brush.raw() ensure HDC and HBRUSH are valid.
    unsafe {
        FillRect(
            dc.raw(),
            &RECT {
                left: x,
                top: y,
                right: x + w,
                bottom: y + h,
            },
            brush.raw(),
        );
    }
}
