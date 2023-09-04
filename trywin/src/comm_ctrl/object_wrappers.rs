// Windows object wrappers with a documented safety model
//
// Many wrappers live in submodules to prevent accidental access to
// the interior; access must be through unsafe raw()

use std::{
    cell::Cell,
    ffi::c_void,
    mem::size_of,
    panic::{catch_unwind, AssertUnwindSafe},
    process::abort,
    rc::Rc,
    result::Result,
};
use windows::{
    core,
    core::*,
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::*,
        UI::{Controls::*, Shell::*, WindowsAndMessaging::*},
    },
};

use super::Error;

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

mod created_window {
    use super::*;

    pub struct CreatedWindow(Rc<Cell<HWND>>);

    impl CreatedWindow {
        /// # Safety
        /// * `parent` must either be valid or null.
        /// * If `control_class` is Some, then it must be a comctl32 class.
        /// * The class name "general_window" is reserved for use by this function.
        //
        // TODO: better name for "general_window" that's less likely to clash.
        pub unsafe fn new<T: WindowProc + 'static>(
            window_proc: T,
            window_name: &str,
            window_style: WINDOW_STYLE,
            window_ex_style: WINDOW_EX_STYLE,
            parent: HWND,
            control_class: Option<&str>,
            x: Option<i32>,
            y: Option<i32>,
            w: Option<i32>,
            h: Option<i32>,
        ) -> Result<Self, Error> {
            const WINDOW_CLASS: PCWSTR = w!("general_window");

            let instance = GetModuleHandleA(None)?;

            if control_class.is_some() {
                // TODO: add more flags
                InitCommonControlsEx(&INITCOMMONCONTROLSEX {
                    dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
                    dwICC: ICC_STANDARD_CLASSES,
                });
            } else if GetClassInfoExW(instance, WINDOW_CLASS, &mut WNDCLASSEXW::default()).is_err()
            {
                let atom = RegisterClassExW(&WNDCLASSEXW {
                    cbSize: size_of::<WNDCLASSEXW>() as u32,
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(static_wndproc::<T>),
                    cbClsExtra: 0,
                    cbWndExtra: 0,
                    hInstance: instance.into(),
                    hIcon: Default::default(),
                    hCursor: Default::default(),
                    hbrBackground: Default::default(),
                    lpszMenuName: PCWSTR::null(),
                    lpszClassName: WINDOW_CLASS,
                    hIconSm: Default::default(),
                });
                if atom == 0 {
                    Err(core::Error::from_win32())?;
                }
            }

            let hwnd = Rc::new(Cell::new(HWND(0)));
            let state = StaticWndprocState::new(hwnd.clone(), window_proc);
            let mut state = Some(Box::into_raw(Box::new(state)) as *const c_void);

            let created_hwnd = CreateWindowExW(
                window_ex_style,
                if let Some(cls) = control_class {
                    WideZString::new(cls).pzwstr()
                } else {
                    WINDOW_CLASS
                },
                WideZString::new(window_name).pzwstr(),
                window_style,
                x.unwrap_or(CW_USEDEFAULT),
                y.unwrap_or(CW_USEDEFAULT),
                w.unwrap_or(CW_USEDEFAULT),
                h.unwrap_or(CW_USEDEFAULT),
                parent,
                None,
                instance,
                if control_class.is_some() {
                    None
                } else {
                    Some(state.take().unwrap())
                },
            );
            if created_hwnd == Default::default() {
                Err(core::Error::from_win32())?;
            }

            if control_class.is_some() {
                hwnd.replace(created_hwnd);
                SetWindowSubclass(
                    created_hwnd,
                    Some(static_subclass_wndproc::<T>),
                    0,
                    state.take().unwrap() as usize,
                );
            }

            Ok(Self(hwnd))
        }

        /// # Safety
        ///
        /// Caller must ensure that `hwnd` is either valid or null.
        /// If it is null, then caller may set it to non-null at
        /// a later time. If `hwnd` is ever destroyed, then caller
        /// must set `hwnd` to null. Once it has been null a second
        /// time, it must never be non-null again. `[StaticWndprocState]`
        /// fulfills these requirements.
        ///
        /// `OwnedWindow`'s drop handler calls `[DestroyWindow]`
        /// on `hwnd` when it is not null.
        pub unsafe fn from_hwnd(hwnd: Rc<Cell<HWND>>) -> Self {
            Self(hwnd)
        }

        /// # Safety
        ///
        /// * All calls return the same handle, except that it may progress
        ///   through the sequence null -> valid -> null.
        /// * Callers must not use hwnd after it is destroyed.
        /// * Callers may destroy hwnd.
        pub unsafe fn hwnd(&self) -> HWND {
            self.0.get()
        }
    }

    impl Drop for CreatedWindow {
        fn drop(&mut self) {
            let hwnd = self.0.get();
            if hwnd != HWND(0) {
                // Safety: self.0 is valid. Caller of OwnedWindow::new
                //         is responsible for setting hwnd to null.
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            }
        }
    }
}
pub use created_window::*;

pub mod window_proc {
    use super::*;

    pub trait WindowProc {
        /// # Safety
        ///
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
        hwnd: Rc<Cell<HWND>>,
        entry_count: Cell<u32>,
        destroy_this: Cell<bool>,
    }

    impl<T> StaticWndprocState<T> {
        /// # Safety
        ///
        /// * Caller must ensure that `hwnd` is either valid or null.
        ///   If it is not null, then it must be the same HWND that will be
        ///   passed to `static_wndproc` or `static_subclass_wndproc`.
        /// * Caller ensures that it will never change `hwnd` once
        ///   `static_wndproc` or `static_subclass_wndproc` execute.
        /// * If `static_wndproc` is used, then it will set `hwnd` while
        ///   processing `WM_NCCREATE`, unless `hwnd` is already non-null.
        /// * Both `static_wndproc` and `static_subclass_wndproc` will
        ///   set it to null while processing `WM_NCDESTROY`.
        pub unsafe fn new(hwnd: Rc<Cell<HWND>>, window_proc: T) -> Self {
            Self {
                window_proc,
                hwnd,
                entry_count: Cell::new(0),
                destroy_this: Cell::new(false),
            }
        }
    }

    /// # Safety
    ///
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
            // Safety: hwnd never changes once set, except back to null.
            if (*p).hwnd.get() == HWND(0) {
                (*p).hwnd.set(handle);
            }
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
            (*p).hwnd.set(HWND(0));
        }

        // Track recursion depth
        (*p).entry_count.set((*p).entry_count.get() - 1);

        // Destroy p if scheduled and we're the last call
        if (*p).entry_count.get() == 0 && (*p).destroy_this.get() {
            drop(Box::from_raw(p as *mut StaticWndprocState<T>));
        }

        res.unwrap_or(LRESULT(0))
    }

    /// # Safety
    ///
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
            (*p).hwnd.set(HWND(0));
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
