use trywin::{comm_ctrl, Window};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        DispatchMessageA, GetMessageA, MSG, WS_CLIPCHILDREN, WS_EX_CONTROLPARENT,
        WS_EX_OVERLAPPEDWINDOW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    },
};

fn main() -> Result<(), comm_ctrl::Error> {
    unsafe {
        let window = trywin::comm_ctrl::WindowImpl::new(
            WS_OVERLAPPEDWINDOW | WS_VISIBLE | WS_CLIPCHILDREN,
            WS_EX_OVERLAPPEDWINDOW | WS_EX_CONTROLPARENT,
            HWND(0),
            None,
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

        window
            .create_child(trywin::ChildType::Button)?
            .bounds(Some((100, 50)), Some((100, 40)))?
            .text("A &Button")?;

        // b.background(Color(127, 0, 127, 255))?;

        let mut message = MSG::default();

        while GetMessageA(&mut message, None, 0, 0).into() {
            DispatchMessageA(&message);
        }

        Ok(())
    }
}
