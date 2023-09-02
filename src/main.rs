use trywin::{
    comm_ctrl::{self, System},
    Color, EditOptions, Window, WindowSystem,
};

fn make<WS: WindowSystem>(ws: &WS) -> Result<WS::Window, WS::Error> {
    let window = ws.main_window()?.text("Hello, world!")?.on_close(|w| {
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
        .on_destroy(|w| w.system().exit_loop().unwrap())?;

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
    let _w = make(&System)?;
    System.event_loop()?;
    Ok(())
}
