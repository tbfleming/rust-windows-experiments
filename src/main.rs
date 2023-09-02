use trywin::{comm_ctrl, Color, EditOptions, Window};
use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{
        DispatchMessageA, GetMessageA, PostQuitMessage, MSG, WS_CLIPCHILDREN, WS_EX_CONTROLPARENT,
        WS_EX_OVERLAPPEDWINDOW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    },
};

fn make() -> Result<trywin::comm_ctrl::Window, comm_ctrl::Error> {
    let window = unsafe {
        trywin::comm_ctrl::WindowImpl::new(
            WS_OVERLAPPEDWINDOW | WS_VISIBLE | WS_CLIPCHILDREN,
            WS_EX_OVERLAPPEDWINDOW | WS_EX_CONTROLPARENT,
            HWND(0),
            None,
            None,
            None,
            None,
            None,
        )
    }?
    .text("Hello, world!")?
    .on_close(|w: &trywin::comm_ctrl::Window| {
        w.destroy().unwrap();
    })?;

    window
        .create_child(trywin::ChildType::Custom)?
        .bounds(Some((10, 10)), Some((50, 50)))?
        .background(Color(127, 0, 127, 255))?;

    window
        .create_child(trywin::ChildType::Button)?
        .bounds(Some((100, 50)), Some((100, 40)))?
        .text("A &Button")?
        .on_destroy(|_| unsafe { PostQuitMessage(0) })?;

    window
        .create_child(trywin::ChildType::Edit(EditOptions {
            border: true,
            hscroll: false,
            vscroll: true,
            auto_hscroll: false,
            auto_vscroll: true,
            center: false,
            lower_case: false,
            multiline: true,
            password: false,
            readonly: false,
            uppercase: false,
            want_return: true,
        }))?
        .bounds(Some((100, 100)), Some((200, 100)))?
        .text("Here is some text and some more and more\r\nAnother line")?; // TODO: newline translation

    Ok(window)
}

fn main() -> Result<(), comm_ctrl::Error> {
    let _w = make()?;

    let mut message = MSG::default();
    unsafe {
        while GetMessageA(&mut message, None, 0, 0).into() {
            DispatchMessageA(&message);
        }
    }

    Ok(())
}
