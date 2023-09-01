use std::mem::size_of;
use trywin::{comm_ctrl, Color, Window};
use windows::Win32::{
    Foundation::HWND,
    UI::{
        Controls::{InitCommonControlsEx, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX},
        WindowsAndMessaging::{
            DispatchMessageA, GetMessageA, MSG, WS_CLIPCHILDREN, WS_EX_CONTROLPARENT,
            WS_EX_OVERLAPPEDWINDOW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        },
    },
};

fn main() -> Result<(), comm_ctrl::Error> {
    unsafe {
        InitCommonControlsEx(&INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_STANDARD_CLASSES,
        });

        let window = trywin::comm_ctrl::WindowImpl::new(
            WS_OVERLAPPEDWINDOW | WS_VISIBLE | WS_CLIPCHILDREN,
            WS_EX_OVERLAPPEDWINDOW | WS_EX_CONTROLPARENT,
            HWND(0),
            None,
            None,
            None,
            None,
        )?;
        window.text("Hello, world!")?;
        window.on_close(|w: &trywin::comm_ctrl::Window| {
            println!("on_close!!!");
            w.destroy().unwrap();
        });
        // window.on_close(|w| {
        //     w.background(Color(0, 127, 0, 255)).unwrap();
        // });

        let b = window.create_child()?;
        b.bounds(Some((100, 100)), Some((400, 400)))?;
        b.background(Color(127, 0, 127, 255))?;


        let mut message = MSG::default();

        while GetMessageA(&mut message, None, 0, 0).into() {
            DispatchMessageA(&message);
        }

        Ok(())
    }
}
