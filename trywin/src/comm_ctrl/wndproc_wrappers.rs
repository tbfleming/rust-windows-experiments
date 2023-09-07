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
        System::LibraryLoader::*,
        UI::{Controls::*, Shell::*, WindowsAndMessaging::*},
    },
};

use super::object_wrappers::Error;
use crate::comm_ctrl::object_wrappers::WideZString;

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
                    lpfnWndProc: Some(static_wndproc),
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
                    Some(static_subclass_wndproc),
                    0,
                    state.take().unwrap() as usize,
                );
            }

            Ok(Self(hwnd))
        }

        /// # Safety
        ///
        /// * All calls return the same handle, or null if it
        ///   has been destroyed.
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
        unsafe fn wndproc(
            &self,
            commctrl: bool,
            hwnd: HWND,
            message: u32,
            wparam: WPARAM,
            lparam: LPARAM,
            default: fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
        ) -> LRESULT;
    }

    pub struct StaticWndprocState {
        window_proc: Box<dyn WindowProc>,
        hwnd: Rc<Cell<HWND>>,
        entry_count: Cell<u32>,
        destroy_this: Cell<bool>,
    }

    impl StaticWndprocState {
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
        pub unsafe fn new<WP: WindowProc + 'static>(hwnd: Rc<Cell<HWND>>, window_proc: WP) -> Self {
            Self {
                window_proc: Box::new(window_proc),
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
    pub unsafe extern "system" fn static_wndproc(
        handle: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // Get p or immediately return if it's null.
        let p: *const StaticWndprocState;
        if message == WM_NCCREATE {
            p = (*(lparam.0 as *const CREATESTRUCTW)).lpCreateParams as *const StaticWndprocState;
            SetWindowLongPtrW(handle, GWLP_USERDATA, p as isize);
            // Safety: hwnd never changes once set, except back to null.
            if (*p).hwnd.get() == HWND(0) {
                (*p).hwnd.set(handle);
            }
        } else {
            p = GetWindowLongPtrW(handle, GWLP_USERDATA) as *const StaticWndprocState;
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
            drop(Box::from_raw(p as *mut StaticWndprocState));
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
    pub unsafe extern "system" fn static_subclass_wndproc(
        handle: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _uidsubclass: usize,
        dwrefdata: usize,
    ) -> LRESULT {
        let p = dwrefdata as *const StaticWndprocState;

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
                    true,
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
            RemoveWindowSubclass(handle, Some(static_subclass_wndproc), 0);
            (*p).hwnd.set(HWND(0));
        }

        // Track recursion depth
        (*p).entry_count.set((*p).entry_count.get() - 1);

        // Destroy p if scheduled and we're the last call
        if (*p).entry_count.get() == 0 && (*p).destroy_this.get() {
            drop(Box::from_raw(p as *mut StaticWndprocState));
        }

        res.unwrap_or(LRESULT(0))
    }
}
pub use window_proc::*;
