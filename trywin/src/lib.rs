use closure_attr::Downgrade;
use std::{fs::File, io::BufWriter, path::Path};

pub mod comm_ctrl;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color(pub u8, pub u8, pub u8, pub u8);

#[non_exhaustive]
#[derive(Clone, Debug)]
pub enum ChildType {
    Custom,
    Button,
    DefaultButton, // TODO: move into Button?
    Checkbox,
    TristateCheckbox,
    Groupbox,
    Radio,
    Edit(EditOptions),
}

#[derive(Clone, Debug, Default)]
pub struct EditOptions {
    pub border: bool,
    pub hscroll: bool,
    pub vscroll: bool,
    pub auto_hscroll: bool,
    pub auto_vscroll: bool,
    pub center: bool,
    pub lower_case: bool,
    pub multiline: bool,
    pub password: bool,
    pub readonly: bool,
    pub uppercase: bool,
    pub want_return: bool,
}

pub trait WindowSystem: Clone + Downgrade + 'static {
    type Error: std::error::Error;
    type Window: Window<Self>;
    type Child: Window<Self>;

    fn new_main(&self) -> Result<Self::Window, Self::Error>;
    fn event_loop(&self) -> Result<(), Self::Error>;
    fn exit_loop(&self) -> Result<(), Self::Error>;
}

pub trait Window<WS: WindowSystem>: Clone + Downgrade + 'static {
    fn system(&self) -> WS;
    fn destroy(&self) -> Result<(), WS::Error>;
    fn new_child(&self, ty: ChildType) -> Result<WS::Child, WS::Error>;

    fn text(self, text: &str) -> Result<Self, WS::Error>;
    fn bounds(
        self,
        upper_left: Option<(i32, i32)>,
        size: Option<(i32, i32)>,
    ) -> Result<Self, WS::Error>;

    // TODO: standard color support (e.g. COLOR_BTNFACE)
    fn background(self, color: Color) -> Result<Self, WS::Error>;

    // TODO: option to not activate and to not go on the taskbar
    fn move_offscreen(self) -> Result<Self, WS::Error>;
    fn visible(self, visible: bool) -> Result<Self, WS::Error>;
    fn redraw(self) -> Result<Self, WS::Error>;
    fn snapshot(&self) -> Result<Bitmap, WS::Error>;

    fn on_close<F: FnMut() + 'static>(&self, callback: F) -> Result<&Self, WS::Error>;
    fn on_destroy<F: FnMut() + 'static>(&self, callback: F) -> Result<&Self, WS::Error>;
    // TODO: mouse, set cursor
}

#[derive(Clone, Debug, Default)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,

    /// 0xAABBGGRR, length = width * height
    pub data: Vec<u32>,
}

impl Bitmap {
    // TODO: error type
    pub fn save_png(&self, path: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
        let mut png =
            png::Encoder::new(BufWriter::new(File::create(path)?), self.width, self.height);
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        let mut writer = png.write_header()?;
        writer.write_image_data(bytemuck::cast_slice(&self.data))?;
        Ok(())
    }
}
