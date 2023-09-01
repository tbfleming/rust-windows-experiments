#![allow(dead_code, unused_imports)]

use std::{
    borrow::BorrowMut,
    cell::RefCell,
    mem::size_of,
    ops::{Deref, DerefMut},
    pin::Pin,
    rc::Rc,
};

use trywin::{comm_ctrl, Window};
use windows::{
    // core::*,
    Win32::Foundation::*,
    Win32::Graphics::Gdi::ValidateRect,
    Win32::UI::WindowsAndMessaging::*,
    Win32::{
        System::LibraryLoader::GetModuleHandleA,
        UI::Controls::{InitCommonControlsEx, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX},
    },
};

fn main() -> Result<(), comm_ctrl::Error> {
    unsafe {
        InitCommonControlsEx(&INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_STANDARD_CLASSES,
        });

        let window = trywin::comm_ctrl::WindowImpl::new(
            "ABCD",
            "efgh",
            CS_HREDRAW | CS_VREDRAW,
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            WS_EX_OVERLAPPEDWINDOW,
            None,
            None,
            None,
            None,
        )?;
        window.on_close(Some(|w: &trywin::comm_ctrl::Window| {
            println!("on_close!!!");
            w.destroy().unwrap();
        }))?;

        let mut message = MSG::default();

        while GetMessageA(&mut message, None, 0, 0).into() {
            DispatchMessageA(&message);
        }

        Ok(())
    }
}
