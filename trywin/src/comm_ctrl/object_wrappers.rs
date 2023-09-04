// Windows object wrappers with a documented safety model
//
// Many wrappers live in submodules to prevent accidental access to
// the interior; access must be through unsafe raw()

use std::{
    cell::Cell,
    panic::{catch_unwind, AssertUnwindSafe},
    process::abort,
};
use windows::{
    core,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        UI::{Shell::*, WindowsAndMessaging::*},
    },
};

use super::Error;

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

mod owned_window {
    use super::*;

    pub struct OwnedWindow(HWND);

    impl OwnedWindow {
        /// # Safety
        ///
        /// Caller must ensure that hwnd is valid and not null.
        /// This takes ownership and will destroy the window
        /// when it is dropped using `[DestroyWindow]`.
        pub unsafe fn new(hwnd: HWND) -> Self {
            Self(hwnd)
        }
    }

    impl Raw<HWND> for OwnedWindow {
        // Safety: see Raw::raw()
        unsafe fn raw(&self) -> HWND {
            self.0
        }
    }

    impl Drop for OwnedWindow {
        fn drop(&mut self) {
            // Safety: self.0 is valid
            unsafe {
                let _ = DestroyWindow(self.0);
            }
        }
    }
}
pub use owned_window::*;

pub mod window_proc {
    use super::*;

    pub trait WindowProc {
        /// Safety:
        /// * Caller is a window procedure that is currently handling a message.
        ///   It is running in the same thread which created the HWND.
        /// * Caller is providing a valid message.
        /// * Caller sets `commctrl` to true iff the window procedure is a subclass
        ///   procedure of a Windows Common Control Library control.
        /// * Caller ensures that hwnd is valid and not null at the beginning of the call.
        /// * Caller ensures that `&self` is valid for the duration of the call.
        /// * Trait implementer should call `default` for any unhandled messages.
        /// * It is OK for the trait implementer to call user-provided callbacks. They
        ///   may destroy the HWND directly or indirectly, e.g. by destroying
        ///   a parent HWND. Both caller and implementer need to safely handle this.
        /// * Caller must not pass an invalid HWND to `default`.
        unsafe fn wndproc<'a>(
            &'a self,
            commctrl: bool,
            hwnd: HWND,
            message: u32,
            wparam: WPARAM,
            lparam: LPARAM,
            default: impl FnOnce(HWND, u32, WPARAM, LPARAM) -> LRESULT + 'a,
        ) -> LRESULT;
    }

    pub struct StaticWndprocState<T> {
        window_proc: T,
        entry_count: Cell<u32>,
        destroy_this: Cell<bool>,
    }

    impl<T> From<T> for StaticWndprocState<T> {
        fn from(window_proc: T) -> Self {
            Self {
                window_proc,
                entry_count: Cell::new(0),
                destroy_this: Cell::new(false),
            }
        }
    }

    /// Safety:
    /// * Let `p` be a `*mut StaticWndprocState<T>` obtained from
    ///   `[Box::into_raw]`. `static_wndproc` owns `p` and will eventually
    ///   release it using `drop(Box::from_raw(p))`. `[Box::into_raw]` must
    ///   have been called on the same thread which created HWND.
    /// * `CREATESTRUCTW::lpCreateParams` must be `p`; it can't be null.
    /// * `GWLP_USERDATA` must either be null or be `p`.
    /// * Must only be called by the Windows API.
    /// * The Windows API guarantees that it will only call this function in the
    ///   same thread that created the HWND.
    pub unsafe extern "system" fn static_wndproc<T: WindowProc + 'static>(
        handle: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // Get p or immediately return if it's null.
        let p: *const StaticWndprocState<T>;
        if message == WM_NCCREATE {
            p = (*(lparam.0 as *const CREATESTRUCTW)).lpCreateParams
                as *const StaticWndprocState<T>;
            SetWindowLongPtrW(handle, GWLP_USERDATA, p as isize);
        } else {
            p = GetWindowLongPtrW(handle, GWLP_USERDATA) as *const StaticWndprocState<T>;
            if p.is_null() {
                return DefWindowProcW(handle, message, wparam, lparam);
            }
        }

        // Track recursion depth
        let Some(c) = (*p).entry_count.get().checked_add(1) else {
            abort();
        };
        (*p).entry_count.set(c);

        // Call the callback
        let res = catch_unwind({
            let p = AssertUnwindSafe(p);
            move || {
                (**p).window_proc.wndproc(
                    false,
                    handle,
                    message,
                    wparam,
                    lparam,
                    |hwnd, message, wparam, lparam| DefWindowProcW(hwnd, message, wparam, lparam),
                )
            }
        });

        if message == WM_NCDESTROY || res.is_err() {
            // Schedule p destruction and prevent further calls to wndproc
            (*p).destroy_this.set(true);
            SetWindowLongPtrW(handle, GWLP_USERDATA, 0);
        }

        // Track recursion depth
        (*p).entry_count.set((*p).entry_count.get() - 1);

        // Destroy p if scheduled and we're the last call
        if (*p).entry_count.get() == 0 && (*p).destroy_this.get() {
            drop(Box::from_raw(p as *mut StaticWndprocState<T>));
        }

        res.unwrap_or(LRESULT(0))
    }

    /// Safety:
    /// * Must be registered as subclass 0.
    /// * Let `p` be a `*mut StaticWndprocState<T>` obtained from
    ///   `[Box::into_raw]`. `static_subclass_wndproc` owns `p` and will eventually
    ///   release it using `drop(Box::from_raw(p))`. `[Box::into_raw]` must
    ///   have been called on the same thread which created HWND.
    /// * dwrefdata must be `p`. It can't be null.
    /// * Must only be called by the Windows API.
    /// * The Windows API guarantees that it will only call this function in the
    ///   same thread that created the HWND.
    pub unsafe extern "system" fn static_subclass_wndproc<T: WindowProc + 'static>(
        handle: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _uidsubclass: usize,
        dwrefdata: usize,
    ) -> LRESULT {
        let p = dwrefdata as *const StaticWndprocState<T>;

        // Track recursion depth
        let Some(c) = (*p).entry_count.get().checked_add(1) else {
            abort();
        };
        (*p).entry_count.set(c);

        // Call the callback
        let res = catch_unwind({
            let p = AssertUnwindSafe(p);
            move || {
                (**p).window_proc.wndproc(
                    false,
                    handle,
                    message,
                    wparam,
                    lparam,
                    |hwnd, message, wparam, lparam| DefSubclassProc(hwnd, message, wparam, lparam),
                )
            }
        });

        if message == WM_NCDESTROY || res.is_err() {
            // Schedule p destruction and prevent further calls to wndproc
            (*p).destroy_this.set(true);
            RemoveWindowSubclass(handle, Some(static_subclass_wndproc::<T>), 0);
        }

        // Track recursion depth
        (*p).entry_count.set((*p).entry_count.get() - 1);

        // Destroy p if scheduled and we're the last call
        if (*p).entry_count.get() == 0 && (*p).destroy_this.get() {
            drop(Box::from_raw(p as *mut StaticWndprocState<T>));
        }

        res.unwrap_or(LRESULT(0))
    }
}
pub use window_proc::*;

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
