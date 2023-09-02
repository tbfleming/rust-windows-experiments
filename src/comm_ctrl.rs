// #![allow(dead_code, unused_imports, unused_variables)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::too_many_arguments)]

use std::{
    cell::{OnceCell, RefCell},
    marker::PhantomPinned,
    mem::size_of,
    pin::Pin,
    rc::{Rc, Weak},
};
use thiserror::Error;
use windows::{
    core,
    core::w,
    core::PCWSTR,
    Win32::Foundation::*,
    Win32::System::LibraryLoader::GetModuleHandleA,
    Win32::{
        Graphics::Gdi::{
            BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, InvalidateRect,
            PAINTSTRUCT,
        },
        UI::{
            Controls::{InitCommonControlsEx, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX},
            Shell::{DefSubclassProc, SetWindowSubclass},
            WindowsAndMessaging::*,
        },
    },
};

use crate::{ChildType, Color, EditOptions, WindowSystem};

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0}")]
    Windows(#[from] core::Error),

    #[error("Window has been destroyed")]
    Destroyed,
}

struct WideZString(Vec<u16>);

impl WideZString {
    fn new(s: &str) -> Self {
        Self(s.encode_utf16().chain(Some(0)).collect())
    }

    fn pzwstr(&self) -> PCWSTR {
        PCWSTR(self.0.as_ptr())
    }
}

impl From<&str> for WideZString {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

#[derive(Clone, Debug, Default)]
pub struct System;

impl System {
    pub fn new() -> Self {
        Self {}
    }
}

impl WindowSystem for System {
    type Error = Error;
    type Window = Window;

    fn main_window(&self) -> Result<Self::Window, Error> {
        unsafe {
            Ok(WindowImpl::new(
                WS_OVERLAPPEDWINDOW | WS_VISIBLE | WS_CLIPCHILDREN,
                WS_EX_OVERLAPPEDWINDOW | WS_EX_CONTROLPARENT,
                HWND(0),
                None,
                None,
                None,
                None,
                None,
            )?)
        }
    }

    fn event_loop(&self) -> Result<(), Error> {
        unsafe {
            let mut msg = MSG::default();
            while GetMessageA(&mut msg, HWND(0), 0, 0).into() {
                DispatchMessageA(&msg);
            }
            Ok(())
        }
    }

    fn exit_loop(&self) -> Result<(), Error> {
        unsafe {
            PostQuitMessage(0);
            Ok(())
        }
    }
}

#[derive(Default)]
enum Callback<F> {
    #[default]
    Empty,
    Filled(F),
    Borrowed,
}

struct CallbackCell<F: ?Sized>(RefCell<Callback<Box<F>>>);

impl<F: ?Sized> CallbackCell<F> {
    fn set(&self, f: Option<Box<F>>) {
        *self.0.borrow_mut() = if let Some(f) = f {
            Callback::Filled(f)
        } else {
            Callback::Empty
        };
    }

    fn borrow(&self) -> CallbackRef<F> {
        let curr = std::mem::replace(&mut *self.0.borrow_mut(), Callback::Borrowed);
        if let Callback::Filled(f) = curr {
            CallbackRef(self, Some(f))
        } else {
            CallbackRef(self, None)
        }
    }

    fn with<G: FnOnce(&mut F) -> T, T>(&self, g: G) -> Option<T> {
        self.borrow().1.as_mut().map(|f| g(f))
    }
}

impl<F: ?Sized> Default for CallbackCell<F> {
    fn default() -> Self {
        Self(RefCell::new(Callback::Empty))
    }
}

struct CallbackRef<'a, F: ?Sized>(&'a CallbackCell<F>, Option<Box<F>>);

impl<'a, F: ?Sized> Drop for CallbackRef<'a, F> {
    fn drop(&mut self) {
        let mut cb = self.0 .0.borrow_mut();
        if let Callback::Borrowed = *cb {
            *cb = if let Some(f) = self.1.take() {
                Callback::Filled(f)
            } else {
                Callback::Empty
            };
        }
    }
}

pub type Window = Pin<Rc<WindowImpl>>;

pub struct WindowImpl {
    hwnd: RefCell<HWND>,
    this: OnceCell<Weak<WindowImpl>>,
    events: WindowEvents,
    options: RefCell<WindowOptions>,
    children: RefCell<Vec<Window>>,
    _pin: PhantomPinned,
}

#[derive(Default)]
struct WindowEvents {
    on_close: CallbackCell<dyn FnMut(&Window)>,
    on_destroy: CallbackCell<dyn FnMut(&Window)>,
}

impl WindowEvents {
    fn clear(&self) {
        self.on_close.set(None);
        self.on_destroy.set(None);
    }
}

#[derive(Default)]
struct WindowOptions {
    background: Option<Color>,
}

impl WindowImpl {
    unsafe fn new(
        window_style: WINDOW_STYLE,
        window_ex_style: WINDOW_EX_STYLE,
        parent: HWND,
        control_class: Option<&str>,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<i32>,
        h: Option<i32>,
    ) -> core::Result<Pin<Rc<Self>>> {
        let instance = GetModuleHandleA(None)?;

        if control_class.is_some() {
            InitCommonControlsEx(&INITCOMMONCONTROLSEX {
                dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_STANDARD_CLASSES,
            });
        } else if GetClassInfoExW(instance, w!("general_window"), &mut WNDCLASSEXW::default())
            .is_err()
        {
            let atom = RegisterClassExW(&WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(WindowImpl::static_wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance.into(),
                hIcon: Default::default(),
                hCursor: Default::default(),
                hbrBackground: Default::default(),
                lpszMenuName: PCWSTR::null(),
                lpszClassName: w!("general_window"),
                hIconSm: Default::default(),
            });
            if atom == 0 {
                return Err(core::Error::from_win32());
            }
        }

        let window = Rc::new(WindowImpl {
            hwnd: Default::default(),
            this: Default::default(),
            events: Default::default(),
            options: Default::default(),
            children: Default::default(),
            _pin: PhantomPinned,
        });
        window.this.set(Rc::downgrade(&window)).unwrap();

        // Side effect: static_wndproc sets window.hwnd, but not if control_class is Some.
        let hwnd = CreateWindowExW(
            window_ex_style,
            WideZString::new(if let Some(cls) = control_class {
                cls
            } else {
                "general_window"
            })
            .pzwstr(),
            w!(""),
            window_style,
            x.unwrap_or(CW_USEDEFAULT),
            y.unwrap_or(CW_USEDEFAULT),
            w.unwrap_or(CW_USEDEFAULT),
            h.unwrap_or(CW_USEDEFAULT),
            parent,
            None,
            instance,
            Some(&*window as *const _ as _),
        );
        if hwnd == Default::default() {
            return Err(core::Error::from_win32());
        }

        if control_class.is_some() {
            window.hwnd.replace(hwnd);
            SetWindowSubclass(
                hwnd,
                Some(WindowImpl::static_subclass_wndproc),
                0,
                &*window as *const _ as _,
            );
        }

        Ok(Pin::new_unchecked(window))
    }

    fn destroy(&self) -> core::Result<()> {
        unsafe {
            let handle = self.hwnd();
            println!("drop handle: {:?}", handle);
            if handle != Default::default() {
                // wndproc will set self.hwnd to null
                DestroyWindow(handle)
            } else {
                Ok(())
            }
        }
    }

    fn this(&self) -> Option<Window> {
        unsafe { Some(Pin::new_unchecked(self.this.get()?.upgrade()?)) }
    }

    fn live(&self) -> bool {
        *self.hwnd.borrow() != Default::default()
    }

    fn check_live(&self) -> Result<(), Error> {
        if !self.live() {
            Err(Error::Destroyed)
        } else {
            Ok(())
        }
    }

    fn set_callback<F: ?Sized>(&self, cell: &CallbackCell<F>, f: Box<F>) {
        if self.live() {
            cell.set(Some(f));
        }
    }

    /// Get raw handle. May be NULL.
    ///
    /// # Thread Safety
    ///
    /// Although WindowImpl is not Send or Sync, HWND is. If you move it to another
    /// thread, then you must account for blocking during Win32 API calls so you can avoid
    /// deadlock. Handle lifetime (below) becomes even more challenging.
    ///
    /// # Safety
    ///
    /// Caller must not destroy use handle after it's been destroyed. It is OK to call
    /// [DestroyWindow].
    pub unsafe fn hwnd(&self) -> HWND {
        *self.hwnd.borrow()
    }

    // safety:
    // * self.hwnd must be valid and not null
    // * WindowImpl still exists, but we could be in drop(), so self.this() could be None.
    unsafe fn wndproc(
        &self,
        subclassed: bool,
        message: u32,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Option<LRESULT> {
        // println!("message: {}", message);
        match message {
            WM_PAINT => {
                if subclassed {
                    return None;
                }
                // println!("WM_PAINT");
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(self.hwnd(), &mut ps);
                if let Some(color) = self.options.borrow().background {
                    let brush = CreateSolidBrush(COLORREF(
                        (color.0 as u32) | ((color.1 as u32) << 8) | ((color.2 as u32) << 16),
                    ));
                    let mut rect = Default::default();
                    if GetClientRect(self.hwnd(), &mut rect).is_ok() {
                        FillRect(hdc, &rect, brush);
                    }
                    DeleteObject(brush);
                }
                EndPaint(self.hwnd(), &ps);
                Some(LRESULT(0))
            }
            WM_CLOSE => {
                println!("WM_CLOSE");
                if let Some(this) = self.this() {
                    self.events.on_close.with(|f| f(&this));
                }
                Some(LRESULT(0))
            }
            WM_DESTROY => {
                println!("WM_DESTROY");
                if let Some(this) = self.this() {
                    self.events.on_destroy.with(|f| f(&this));
                }
                None
            }
            WM_NCDESTROY => {
                println!("WM_NCDESTROY");
                *self.hwnd.borrow_mut() = Default::default();
                self.children.borrow_mut().clear();
                self.events.clear(); // Clean up any callbacks that hold a circular Rc to us
                None
            }
            _ => None,
        }
    }

    // Safety:
    // * CREATESTRUCTW::lpCreateParams must point to the owning Window instance.
    // * GWLP_USERDATA must either be null or point to the owning Window instance.
    // * Windows message loops only call this in the same thread that called
    //   CreateWindowExW, no matter what other threads do to the handle.
    unsafe extern "system" fn static_wndproc(
        handle: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // println!("message: {}", message);
        let window;
        if message == WM_NCCREATE {
            // This is almost the first message received by the window. CreateWindowExW
            // hasn't returned yet, so we're responsible for setting window.hwnd.
            // Hopefully we'll never need to handle WM_GETMINMAXINFO, which comes first,
            // since I don't want to thunk wndproc.
            println!("WM_NCCREATE");
            let cs = &*(lparam.0 as *const CREATESTRUCTW);
            window = cs.lpCreateParams as *const WindowImpl;
            (*window).hwnd.replace(handle);
            SetWindowLongPtrW(handle, GWLP_USERDATA, cs.lpCreateParams as isize);
        } else {
            window = GetWindowLongPtrW(handle, GWLP_USERDATA) as *const WindowImpl;
        }
        // println!("window: {:?}", window);
        if window.is_null() {
            println!("window == null");
            DefWindowProcW(handle, message, wparam, lparam)
        } else {
            (*window)
                .wndproc(false, message, wparam, lparam)
                .unwrap_or_else(|| DefWindowProcW(handle, message, wparam, lparam))
        }
    }

    // Safety:
    // * dwrefdata must point to the owning Window instance.
    // * Windows message loops only call this in the same thread that called
    //   CreateWindowExW, no matter what other threads do to the handle.
    unsafe extern "system" fn static_subclass_wndproc(
        handle: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _uidsubclass: usize,
        dwrefdata: usize,
    ) -> LRESULT {
        // println!("message: {}", message);
        let window = dwrefdata as *const WindowImpl;
        // println!("control window: {:?}", window);
        if window.is_null() {
            println!("window == null");
            DefSubclassProc(handle, message, wparam, lparam)
        } else {
            (*window)
                .wndproc(true, message, wparam, lparam)
                .unwrap_or_else(|| DefSubclassProc(handle, message, wparam, lparam))
        }
    }
}

impl Drop for WindowImpl {
    fn drop(&mut self) {
        if let Err(e) = self.destroy() {
            eprintln!("Window::destroy failed in drop handler: {:?}", e);
        }
        print!("drop WindowImpl");
    }
}

fn edit_options(opts: EditOptions) -> WINDOW_STYLE {
    WS_CHILD
        | if opts.border { WS_BORDER } else { WS_CHILD }
        | if opts.hscroll { WS_HSCROLL } else { WS_CHILD }
        | if opts.vscroll { WS_VSCROLL } else { WS_CHILD }
        | WINDOW_STYLE(
            (if opts.auto_hscroll { ES_AUTOHSCROLL } else { 0 }
                | if opts.auto_vscroll { ES_AUTOVSCROLL } else { 0 }
                | if opts.center { ES_CENTER } else { 0 }
                | if opts.lower_case { ES_LOWERCASE } else { 0 }
                | if opts.multiline { ES_MULTILINE } else { 0 }
                | if opts.password { ES_PASSWORD } else { 0 }
                | if opts.readonly { ES_READONLY } else { 0 }
                | if opts.uppercase { ES_UPPERCASE } else { 0 }
                | if opts.want_return { ES_WANTRETURN } else { 0 }) as u32,
        )
}

impl crate::Window<System> for Window {
    type Child = Self;

    fn system(&self) -> System {
        System::new()
    }

    fn destroy(&self) -> Result<(), Error> {
        WindowImpl::destroy(self)?;
        Ok(())
    }

    fn create_child(&self, ty: ChildType) -> Result<Self::Child, Error> {
        self.check_live()?;
        let control = |class, style| -> Result<Self::Child, Error> {
            unsafe {
                Ok(WindowImpl::new(
                    style,
                    Default::default(),
                    self.hwnd(),
                    Some(class),
                    None,
                    None,
                    None,
                    None,
                )?)
            }
        };
        let child = match ty {
            ChildType::Custom => unsafe {
                WindowImpl::new(
                    WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                    Default::default(),
                    self.hwnd(),
                    None,
                    None,
                    None,
                    None,
                    None,
                )?
            },
            ChildType::Button => control(
                "BUTTON",
                WS_VISIBLE | WS_CHILD | WINDOW_STYLE(BS_PUSHBUTTON as u32),
            )?,
            ChildType::DefaultButton => control(
                "BUTTON",
                WS_VISIBLE | WS_CHILD | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
            )?,
            ChildType::Checkbox => control(
                "BUTTON",
                WS_VISIBLE | WS_CHILD | WINDOW_STYLE(BS_CHECKBOX as u32),
            )?,
            ChildType::TristateCheckbox => control(
                "BUTTON",
                WS_VISIBLE | WS_CHILD | WINDOW_STYLE(BS_3STATE as u32),
            )?,
            ChildType::Groupbox => control(
                "BUTTON",
                WS_VISIBLE | WS_CHILD | WINDOW_STYLE(BS_GROUPBOX as u32),
            )?,
            ChildType::Radio => control(
                "BUTTON",
                WS_VISIBLE | WS_CHILD | WINDOW_STYLE(BS_RADIOBUTTON as u32),
            )?,
            ChildType::Edit(opts) => control("EDIT", WS_VISIBLE | WS_CHILD | edit_options(opts))?,
        };
        self.children.borrow_mut().push(child.clone());
        Ok(child)
    }

    fn text(self, text: &str) -> Result<Self, Error> {
        self.check_live()?;
        unsafe {
            SetWindowTextW(self.hwnd(), WideZString::new(text).pzwstr())?;
            Ok(self)
        }
    }

    fn bounds(
        self,
        upper_left: Option<(i32, i32)>,
        size: Option<(i32, i32)>,
    ) -> Result<Self, Error> {
        self.check_live()?;
        unsafe {
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            GetWindowRect(self.hwnd(), &mut rect)?;
            let (mut x, mut y, mut cx, mut cy) = (
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
            );
            if let Some((xx, yy)) = upper_left {
                x = xx;
                y = yy;
            }
            if let Some((w, h)) = size {
                cx = w;
                cy = h;
            }
            SetWindowPos(
                self.hwnd(),
                HWND(0),
                x,
                y,
                cx,
                cy,
                SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE,
            )?;
            Ok(self)
        }
    }

    fn background(self, color: Color) -> Result<Self, Error> {
        self.check_live()?;
        self.options.borrow_mut().background = Some(color);
        self.redraw()
    }

    fn visible(self, visible: bool) -> Result<Self, Error> {
        self.check_live()?;
        unsafe {
            ShowWindow(self.hwnd(), if visible { SW_SHOW } else { SW_HIDE });
        }
        Ok(self)
    }

    fn redraw(self) -> Result<Self, Error> {
        self.check_live()?;
        unsafe {
            InvalidateRect(self.hwnd(), None, true);
        }
        Ok(self)
    }

    fn on_close<F: FnMut(&Self) + 'static>(self, callback: F) -> Result<Self, Error> {
        self.set_callback(&self.events.on_close, Box::new(callback));
        Ok(self)
    }

    fn on_destroy<F: FnMut(&Self) + 'static>(self, callback: F) -> Result<Self, Error> {
        self.set_callback(&self.events.on_destroy, Box::new(callback));
        Ok(self)
    }
}
