use std::rc::Rc;
use trywin::{
    comm_ctrl::{self, System},
    ChildType, Color, EditOptions, Window, WindowSystem,
};

fn make<WS: WindowSystem>(ws: &WS) -> Result<WS::Window, WS::Error> {
    #![allow(unused_variables, dead_code)]

    let window = ws
        .main_window()?
        .text("Hello, world!")?
        .background(Color(128, 128, 128, 0))?;
    let color1 = window
        .create_child(ChildType::Custom)?
        .bounds(Some((10, 10)), Some((50, 50)))?
        .background(Color(127, 0, 127, 255))?;
    let color2 = window
        .create_child(ChildType::Custom)?
        .bounds(Some((70, 10)), Some((50, 50)))?
        .background(Color(0, 127, 127, 255))?;
    let button1 = window
        .create_child(ChildType::Button)?
        .bounds(Some((100, 50)), Some((100, 40)))?
        .text("A &Button 1")?;
    let button2 = window
        .create_child(ChildType::Button)?
        .bounds(Some((100, 100)), Some((100, 40)))?
        .text("A &Button 2")?;
    let edit = window
        .create_child(ChildType::Edit(EditOptions {
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

    struct All<WS: WindowSystem> {
        window: WS::Window,
        color1: WS::Child,
        color2: WS::Child,
        button1: WS::Child,
        button2: WS::Child,
        edit: WS::Child,
    }
    let all = Rc::new(All::<WS> {
        window: window.clone(),
        color1: color1.clone(),
        color2: color2.clone(),
        button1: button1.clone(),
        button2: button2.clone(),
        edit: edit.clone(),
    });

    window.on_close({
        let all = all.clone();
        move || {
            println!("xxxx colors");
            all.color1.destroy().unwrap();
            all.color2.destroy().unwrap();
        }
    })?;

    // window.on_close({
    //     let window = window.clone();
    //     move || {
    //         window.destroy().unwrap();
    //     }
    // })?;

    window.on_destroy({
        let ws = ws.clone();
        move || ws.exit_loop().unwrap()
    })?;

    Ok(window)
}

fn main() -> Result<(), comm_ctrl::Error> {
    let _w = make(&System)?;
    System.event_loop()?;
    Ok(())
}
