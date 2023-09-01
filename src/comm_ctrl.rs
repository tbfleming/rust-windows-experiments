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
    core,
    core::w,
    core::PCWSTR,
    Win32::Foundation::*,
    Win32::Graphics::Gdi::ValidateRect,
    Win32::System::LibraryLoader::GetModuleHandleA,
    Win32::{
        Graphics::Gdi::{
            BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, InvalidateRect,
            RedrawWindow, HDC, PAINTSTRUCT, RDW_ERASE, RDW_INVALIDATE,
        },
        UI::WindowsAndMessaging::*,
    },
};

use crate::Color;

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

pub type Window<'event> = Pin<Rc<WindowImpl<'event>>>;

pub struct WindowImpl<'event> {
    hwnd: RefCell<HWND>,
    this: OnceCell<Weak<WindowImpl<'event>>>,
    events: WindowEvents<'event>,
    options: RefCell<WindowOptions>,
    children: RefCell<Vec<Window<'event>>>,
}

#[derive(Default)]
struct WindowEvents<'event> {
    on_close: CallbackCell<dyn FnMut(&Window<'event>) + 'event>,
}

impl WindowEvents<'_> {
    fn clear(&self) {
        self.on_close.set(None);
    }
}

#[derive(Default)]
struct WindowOptions {
    background: Option<Color>,
}

impl<'event> WindowImpl<'event> {
    // !!! pub
    pub unsafe fn new(
        window_style: WINDOW_STYLE,
        window_ex_style: WINDOW_EX_STYLE,
        parent: HWND,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<i32>,
        h: Option<i32>,
    ) -> core::Result<Pin<Rc<Self>>> {
        let instance = GetModuleHandleA(None)?;

        if GetClassInfoExW(instance, w!("general_window"), &mut WNDCLASSEXW::default()).is_err() {
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
        });
        window.this.set(Rc::downgrade(&window)).unwrap();

        // Side effect: static_wndproc sets window.hwnd
        let hwnd = CreateWindowExW(
            window_ex_style,
            w!("general_window"),
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
    unsafe fn wndproc(&self, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // println!("message: {}", message);
        match message {
            // WM_ERASEBKGND => {
            //     println!("WM_ERASEBKGND");
            //     if let Some(color) = self.options.borrow().background {
            //         let hdc = HDC(wparam.0 as _);
            //         let mut rect = Default::default();
            //         if GetClientRect(self.hwnd(), &mut rect).is_ok() {
            //             return LRESULT(1);
            //         }
            //     }
            //     LRESULT(0)
            // }
            WM_PAINT => {
                println!("WM_PAINT");
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
                self.children.borrow_mut().clear();
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

    fn create_child(&self) -> Result<Self::Child, Self::Error> {
        self.check_live()?;
        let child = unsafe {
            WindowImpl::new(
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                Default::default(),
                self.hwnd(),
                None,
                None,
                None,
                None,
            )?
        };
        self.children.borrow_mut().push(child.clone());
        Ok(child)
    }

    fn text(&self, text: &str) -> Result<&Self, Self::Error> {
        self.check_live()?;
        unsafe {
            SetWindowTextW(self.hwnd(), WideZString::new(text).pzwstr())?;
            Ok(self)
        }
    }

    fn bounds(
        &self,
        upper_left: Option<(i32, i32)>,
        size: Option<(i32, i32)>,
    ) -> Result<&Self, Self::Error> {
        self.check_live()?;
        unsafe {
            let mut rect = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            GetWindowRect(self.hwnd(), &mut rect)?;
            let mut flags =
                SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE;
            if let Some((x, y)) = upper_left {
                rect.left = x;
                rect.top = y;
                flags &= !SWP_NOMOVE;
            }
            if let Some((w, h)) = size {
                rect.right = rect.left + w;
                rect.bottom = rect.top + h;
                flags &= !SWP_NOSIZE;
            }
            SetWindowPos(
                self.hwnd(),
                HWND(0),
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                flags,
            )?;
            Ok(self)
        }
    }

    fn background(&self, color: Color) -> Result<&Self, Self::Error> {
        self.check_live()?;
        self.options.borrow_mut().background = Some(color);
        self.redraw()
    }

    fn show(&self, visible: bool) -> Result<&Self, Self::Error> {
        todo!()
    }

    fn redraw(&self) -> Result<&Self, Self::Error> {
        self.check_live()?;
        unsafe {
            println!("!!!!!!!!!!\n redrawing");
            InvalidateRect(self.hwnd(), None, true);
        }
        Ok(self)
    }

    fn enable(&self, enabled: bool) -> Result<&Self, Self::Error> {
        todo!()
    }

    fn on_close<F: FnMut(&Self) + 'event>(&self, callback: F) {
        self.set_callback(&self.events.on_close, Box::new(callback));
    }
}
