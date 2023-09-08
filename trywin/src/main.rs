use std::{error::Error, path::Path, rc::Rc};

#[closure_attr::with_closure]
fn make<WS: ::trywin::WindowSystem>(ws: WS) -> Result<WS::Window, WS::Error> {
    use ::trywin::*;

    let window = ws
        .new_main()?
        .bounds(None, Some((500, 300)))?
        .text("Hello, world!")?
        .background(Color(128, 128, 128, 0))?;
    let color1 = window
        .new_child(ChildType::Custom)?
        .bounds(Some((10, 10)), Some((50, 50)))?
        .background(Color(255, 0, 0, 255))?;
    let color2 = window
        .new_child(ChildType::Custom)?
        .bounds(Some((70, 10)), Some((50, 50)))?
        .background(Color(0, 255, 0, 255))?;
    let color3 = window
        .new_child(ChildType::Custom)?
        .bounds(Some((130, 10)), Some((50, 50)))?
        .background(Color(0, 0, 255, 255))?;
    let button1 = window
        .new_child(ChildType::Button)?
        .bounds(Some((100, 50)), Some((100, 40)))?
        .text("A &Button 1")?;
    let button2 = window
        .new_child(ChildType::Button)?
        .bounds(Some((100, 100)), Some((100, 40)))?
        .text("A &Button 2")?;
    let edit = window
        .new_child(ChildType::Edit(EditOptions {
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
        .bounds(Some((210, 100)), Some((200, 100)))?
        .text("Here is some text and some more and more\r\nAnother line")?; // TODO: newline translation

    #[allow(dead_code)]
    struct All<WS: WindowSystem> {
        window: WS::Window,
        color1: WS::Child,
        color2: WS::Child,
        color3: WS::Child,
        button1: WS::Child,
        button2: WS::Child,
        edit: WS::Child,
    }
    #[allow(unused_variables)]
    let all = Rc::new(All::<WS> {
        window: window.clone(),
        color1: color1.clone(),
        color2: color2.clone(),
        color3: color3.clone(),
        button1: button1.clone(),
        button2: button2.clone(),
        edit: edit.clone(),
    });

    window.on_close(
        #[closure(weak window)]
        move || {
            window.destroy().unwrap();
        },
    )?;

    window.on_destroy(
        #[closure(weak ws)]
        move || ws.exit_loop().unwrap(),
    )?;

    Ok(window)
}

fn _make2<WS2>(_ws: WS2)
where
    WS2: Clone + trywin::WindowSystem,
{
    //
}

fn main() -> Result<(), Box<dyn Error>> {
    use trywin::{comm_ctrl::System, Window, WindowSystem};

    let _w = make(System.clone())?;
    // let _w = _w.move_offscreen()?;
    let _w = _w.visible(true)?;
    _w.snapshot()?.save_png(Path::new("snapshot.png"))?;
    System.event_loop()?;
    Ok(())
}
