#![allow(clippy::too_many_arguments)]

use std::{cell::RefCell, mem::size_of, rc::Rc, result::Result};
use windows::{
    core,
    Win32::Storage::Xps::*,
    Win32::{Foundation::*, Graphics::Gdi::*, UI::WindowsAndMessaging::*},
};

use crate::{Bitmap, ChildType, Color, EditOptions, WindowSystem};

pub mod object_wrappers;
use object_wrappers::{Error, *};

pub mod wndproc_wrappers;
use wndproc_wrappers::*;

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
    type Child = Window;

    fn new_main(&self) -> Result<Self::Window, Error> {
        unsafe {
            WindowImpl::new(
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                WS_EX_OVERLAPPEDWINDOW | WS_EX_CONTROLPARENT,
                HWND(0),
                None,
                None,
                None,
                None,
                None,
            )
        }
    }

    // TODO: keyboard, dialog, redirect control notifications
    fn event_loop(&self) -> Result<(), Error> {
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND(0), 0, 0).into() {
                DispatchMessageW(&msg);
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

pub type Window = Rc<WindowImpl>;

pub struct WindowImpl {
    hwnd: CreatedWindow,
    callbacks: Rc<Callbacks>,
}

#[derive(Default)]
struct Callbacks {
    options: RefCell<WindowOptions>,
    on_close: CallbackCell<dyn FnMut()>,
    on_destroy: CallbackCell<dyn FnMut()>,

    // TODO: remove destroyed children from this list
    children: RefCell<Vec<Window>>,
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
    ) -> Result<Rc<Self>, Error> {
        let callbacks = Rc::new(Callbacks::default());
        let hwnd = CreatedWindow::new(
            callbacks.clone(),
            "",
            window_style,
            window_ex_style,
            parent,
            control_class,
            x,
            y,
            w,
            h,
        )?;
        Ok(Rc::new(Self { hwnd, callbacks }))
    }

    fn destroy(&self) -> Result<(), Error> {
        unsafe {
            let handle = self.hwnd.hwnd();
            if handle != Default::default() {
                // wndproc will set self.hwnd to null
                DestroyWindow(handle)?;
            }
            Ok(())
        }
    }

    fn live(&self) -> bool {
        unsafe { self.hwnd.hwnd() != HWND(0) }
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
    /// Caller must not use handle after it's been destroyed. It is OK to call
    /// [DestroyWindow].
    pub unsafe fn hwnd(&self) -> HWND {
        self.hwnd.hwnd()
    }
}

impl Drop for WindowImpl {
    fn drop(&mut self) {
        if let Err(e) = self.destroy() {
            eprintln!("Window::destroy failed in drop handler: {:?}", e);
        }
        // println!("drop WindowImpl");
    }
}

impl Callbacks {
    fn wndproc_impl(
        &self,
        commctrl: bool,
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        default: fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
    ) -> LRESULT {
        // Safety: hwnd is valid until we call any user-provided callbacks
        let raw_hwnd = unsafe { RawHwnd::new(hwnd) };

        match message {
            WM_PAINT => {
                if commctrl {
                    return default(hwnd, message, wparam, lparam);
                }
                // println!("WM_PAINT");
                if let Ok(hdc) = PaintDC::new(&raw_hwnd) {
                    if let Some(color) = self.options.borrow().background {
                        if let Ok(brush) = HBrush::solid(color) {
                            if let Ok((x, y, w, h)) = get_client_rect(&raw_hwnd) {
                                fill_rect(&hdc, &brush, x, y, w, h);
                            }
                        }
                    }
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                // println!("WM_CLOSE");
                self.on_close.with(|f| f());
                LRESULT(0)
            }
            WM_DESTROY => {
                // println!("WM_DESTROY");
                self.on_destroy.with(|f| f());
                default(hwnd, message, wparam, lparam)
            }
            WM_NCDESTROY => {
                // println!("WM_NCDESTROY");
                self.children.borrow_mut().clear();
                default(hwnd, message, wparam, lparam)
            }
            _ => default(hwnd, message, wparam, lparam),
        }
    }
}

impl WindowProc for Rc<Callbacks> {
    unsafe fn wndproc(
        &self,
        commctrl: bool,
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        default: fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
    ) -> LRESULT {
        self.wndproc_impl(commctrl, hwnd, message, wparam, lparam, default)
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
    fn system(&self) -> System {
        System::new()
    }

    fn destroy(&self) -> Result<(), Error> {
        WindowImpl::destroy(self)?;
        Ok(())
    }

    fn new_child(&self, ty: ChildType) -> Result<Window, Error> {
        self.check_live()?;
        let control = |class, style| -> Result<Window, Error> {
            unsafe {
                WindowImpl::new(
                    style,
                    Default::default(),
                    self.hwnd(),
                    Some(class),
                    None,
                    None,
                    None,
                    None,
                )
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
        self.callbacks.children.borrow_mut().push(child.clone());
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
        self.callbacks.options.borrow_mut().background = Some(color);
        self.redraw()
    }

    fn visible(self, visible: bool) -> Result<Self, Error> {
        self.check_live()?;
        unsafe {
            ShowWindow(self.hwnd(), if visible { SW_SHOW } else { SW_HIDE });
        }
        Ok(self)
    }

    fn move_offscreen(self) -> Result<Self, Error> {
        self.check_live()?;
        unsafe {
            SetWindowPos(
                self.hwnd(),
                HWND(0),
                GetSystemMetrics(SM_XVIRTUALSCREEN) + GetSystemMetrics(SM_CXVIRTUALSCREEN) + 10,
                0,
                0,
                0,
                SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE | SWP_NOSIZE,
            )?;
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

    fn snapshot(&self) -> Result<Bitmap, Error> {
        self.check_live()?;
        unsafe {
            let hwnd = RawHwnd::new(self.hwnd());
            let (_, _, w, h) = get_window_rect(&hwnd)?;
            let window_dc = WindowDC::new(&hwnd)?;
            let bm = HBitmap::compatible(&window_dc, w, h)?;
            let memory_dc = MemoryDc::compatible(&window_dc)?;
            select_object(&memory_dc, &bm.gdiobj(), || {
                if PrintWindow(hwnd.raw(), memory_dc.raw(), Default::default()).0 == 0 {
                    Err(core::Error::from_win32())?;
                }
                Ok(())
            })?;
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as _,
                    ..Default::default()
                },
                bmiColors: Default::default(),
            };
            if GetDIBits(
                memory_dc.raw(),
                bm.raw(),
                0,
                h as u32,
                None,
                &mut bmi,
                DIB_RGB_COLORS,
            ) == 0
            {
                Err(core::Error::from_win32())?;
            }
            bmi.bmiHeader.biHeight = -bmi.bmiHeader.biHeight.abs();
            bmi.bmiHeader.biCompression = BI_RGB.0;
            // println!("bmi: {:?}", bmi);
            if bmi.bmiHeader.biBitCount != 32
                || bmi.bmiHeader.biPlanes != 1
                || bmi.bmiHeader.biSizeImage == 0
                || bmi.bmiHeader.biSizeImage & 3 != 0
            {
                Err(Error::UnsupportedBitmapFormat)?;
            }
            let mut bits = vec![0u32; bmi.bmiHeader.biSizeImage as usize / 4];
            if GetDIBits(
                memory_dc.raw(),
                bm.raw(),
                0,
                h as u32,
                Some(bits.as_mut_ptr() as _),
                &mut bmi,
                DIB_RGB_COLORS,
            ) == 0
            {
                Err(core::Error::from_win32())?;
            }
            for pixel in &mut bits {
                *pixel = 0xff000000
                    | ((*pixel & 0xff) << 16)
                    | (*pixel & 0xff00)
                    | ((*pixel & 0xff0000) >> 16);
            }
            Ok(Bitmap {
                width: bmi.bmiHeader.biWidth as u32,
                height: -bmi.bmiHeader.biHeight as u32,
                data: bits,
            })
        }
    }

    fn on_close<F: FnMut() + 'static>(&self, callback: F) -> Result<&Self, Error> {
        self.set_callback(&self.callbacks.on_close, Box::new(callback));
        Ok(self)
    }

    fn on_destroy<F: FnMut() + 'static>(&self, callback: F) -> Result<&Self, Error> {
        self.set_callback(&self.callbacks.on_destroy, Box::new(callback));
        Ok(self)
    }
}
