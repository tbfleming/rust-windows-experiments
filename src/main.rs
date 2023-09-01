use std::mem::size_of;
use trywin::{comm_ctrl, Window};
use windows::Win32::UI::{
    Controls::{InitCommonControlsEx, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX},
    WindowsAndMessaging::{
        DispatchMessageA, GetMessageA, CS_HREDRAW, CS_VREDRAW, MSG, WS_EX_OVERLAPPEDWINDOW,
        WS_OVERLAPPEDWINDOW, WS_VISIBLE,
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
        window.on_close(|w: &trywin::comm_ctrl::Window| {
            println!("on_close!!!");
            w.destroy().unwrap();
        });

        let mut message = MSG::default();

        while GetMessageA(&mut message, None, 0, 0).into() {
            DispatchMessageA(&message);
        }

        Ok(())
    }
}
