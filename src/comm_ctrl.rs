#![allow(dead_code, unused_imports, unused_variables)]
#![allow(clippy::missing_safety_doc)]
#![allow(clippy::too_many_arguments)]

use std::{
    cell::{OnceCell, RefCell},
    mem::size_of,
    pin::Pin,
    rc::{Rc, Weak},
};
use thiserror::Error;
use windows::{
    core, core::PCWSTR, Win32::Foundation::*, Win32::Graphics::Gdi::ValidateRect,
    Win32::System::LibraryLoader::GetModuleHandleA, Win32::UI::WindowsAndMessaging::*,
};

use crate::Color;

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0}")]
    Windows(#[from] core::Error),
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

struct WindowClass(u16);

impl WindowClass {
    unsafe fn register(wc: &WNDCLASSEXW) -> core::Result<Self> {
        unsafe {
            let atom = RegisterClassExW(wc);
            if atom == 0 {
                return Err(core::Error::from_win32());
            }
            Ok(Self(atom))
        }
    }
}

impl Drop for WindowClass {
    fn drop(&mut self) {
        unsafe {
            if let Err(e) = UnregisterClassW(PCWSTR(self.0 as *const u16), None) {
                eprintln!("UnregisterClassW failed: {:?}", e);
            }
        }
    }
}

pub type Window<'event> = Pin<Rc<WindowImpl<'event>>>;

pub struct WindowImpl<'event> {
    hwnd: RefCell<HWND>,
    class: WindowClass, // safety: must outlive hwnd
    this: OnceCell<Weak<WindowImpl<'event>>>,
    events: WindowEvents<'event>,
}

#[derive(Default)]
pub struct WindowEvents<'event> {
    on_close: CallbackCell<dyn FnMut(&Window<'event>) + 'event>,
}

impl WindowEvents<'_> {
    fn clear(&self) {
        self.on_close.set(None);
    }
}

impl<'event> WindowImpl<'event> {
    // !!! pub
    pub unsafe fn new(
        class_name: &str,
        window_name: &str,
        class_style: WNDCLASS_STYLES,
        window_style: WINDOW_STYLE,
        window_ex_style: WINDOW_EX_STYLE,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<i32>,
        h: Option<i32>,
    ) -> core::Result<Pin<Rc<Self>>> {
        let instance = GetModuleHandleA(None)?;

        let class = WindowClass::register(&WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            style: class_style,
            lpfnWndProc: Some(WindowImpl::static_wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance.into(),
            hIcon: Default::default(),
            hCursor: Default::default(),
            hbrBackground: Default::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: WideZString::new(class_name).pzwstr(),
            hIconSm: Default::default(),
        })?;

        let window = Rc::new(WindowImpl {
            hwnd: Default::default(),
            class,
            this: Default::default(),
            events: Default::default(),
        });
        window.this.set(Rc::downgrade(&window)).unwrap();

        // Side effect: static_wndproc sets window.hwnd
        let hwnd = CreateWindowExW(
            window_ex_style,
            PCWSTR(window.class.0 as *const u16),
            WideZString::new(window_name).pzwstr(),
            window_style,
            x.unwrap_or(CW_USEDEFAULT),
            y.unwrap_or(CW_USEDEFAULT),
            w.unwrap_or(CW_USEDEFAULT),
            h.unwrap_or(CW_USEDEFAULT),
            None,
            None,
            instance,
            Some(&*window as *const _ as _),
        );

        if hwnd == Default::default() {
            return Err(core::Error::from_win32());
        }

        Ok(Pin::new(window))
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

    fn this(&self) -> Option<Window<'event>> {
        Some(Pin::new(self.this.get()?.upgrade()?))
    }

    fn live(&self) -> bool {
        *self.hwnd.borrow() != Default::default()
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
    unsafe fn wndproc(&self, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        println!("message: {}", message);
        match message {
            WM_PAINT => {
                println!("WM_PAINT");
                ValidateRect(self.hwnd(), None);
                LRESULT(0)
            }
            WM_CLOSE => {
                println!("WM_CLOSE");
                if let Some(this) = self.this() {
                    self.events.on_close.with(|f| f(&this));
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                println!("WM_DESTROY");
                // TODO: callback instead of PostQuitMessage
                PostQuitMessage(0);
                LRESULT(0)
            }
            WM_NCDESTROY => {
                println!("WM_NCDESTROY");
                *self.hwnd.borrow_mut() = Default::default();
                self.events.clear(); // Clean up any callbacks that hold a circular Rc to us
                LRESULT(0)
            }
            _ => DefWindowProcW(self.hwnd(), message, wparam, lparam),
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
        println!("message: {}", message);
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
        println!("window: {:?}", window);
        if window.is_null() {
            println!("window == null");
            DefWindowProcW(handle, message, wparam, lparam)
        } else {
            (*window).wndproc(message, wparam, lparam)
        }
    }
}

impl<'event> Drop for WindowImpl<'event> {
    fn drop(&mut self) {
        if let Err(e) = self.destroy() {
            eprintln!("Window::destroy failed in drop handler: {:?}", e);
        }
        print!("drop WindowImpl");
    }
}

impl<'event> crate::Window<'event> for Window<'event> {
    type Error = Error;
    type Child = Self;

    fn destroy(&self) -> Result<(), Self::Error> {
        WindowImpl::destroy(self)?;
        Ok(())
    }

    fn create_child(&self) -> Result<&Self::Child, Self::Error> {
        todo!()
    }

    fn text(&self, text: &str) -> Result<&Self, Self::Error> {
        todo!()
    }

    fn bounds(
        &self,
        upper_left: Option<(i32, i32)>,
        size: Option<(i32, i32)>,
    ) -> Result<&Self, Self::Error> {
        todo!()
    }

    fn background(&self, color: Color) -> Result<&Self, Self::Error> {
        todo!()
    }

    fn show(&self, visible: bool) -> Result<&Self, Self::Error> {
        todo!()
    }

    fn enable(&self, enabled: bool) -> Result<&Self, Self::Error> {
        todo!()
    }

    fn on_close<F: FnMut(&Self) + 'event>(&self, callback: F) {
        self.set_callback(&self.events.on_close, Box::new(callback));
    }
}
