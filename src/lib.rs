pub mod comm_ctrl;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Color(pub u8, pub u8, pub u8, pub u8);

pub enum ChildType {
    Custom,
    Button,
    DefaultButton,
    Checkbox,
    TristateCheckbox,
    Groupbox,
    Radio,
    Edit(EditOptions),
}

pub struct ButtonOptions {
    pub pushbutton: bool,
    pub default_pushbutton: bool,
    pub checkbox: bool,
    pub tristate: bool,
}

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

pub trait WindowSystem: Clone + 'static {
    type Error: std::error::Error;
    type Window: Window<Self>;
    type Child: Window<Self>;

    fn main_window(&self) -> Result<Self::Window, Self::Error>;
    fn event_loop(&self) -> Result<(), Self::Error>;
    fn exit_loop(&self) -> Result<(), Self::Error>;
}

pub trait Window<WS: WindowSystem>: Clone + 'static {
    fn system(&self) -> WS;
    fn destroy(&self) -> Result<(), WS::Error>;
    fn create_child(&self, ty: ChildType) -> Result<WS::Child, WS::Error>;

    fn text(self, text: &str) -> Result<Self, WS::Error>;
    fn bounds(
        self,
        upper_left: Option<(i32, i32)>,
        size: Option<(i32, i32)>,
    ) -> Result<Self, WS::Error>;
    fn background(self, color: Color) -> Result<Self, WS::Error>;
    fn visible(self, visible: bool) -> Result<Self, WS::Error>;
    fn redraw(self) -> Result<Self, WS::Error>;

    fn on_close<F: FnMut() + 'static>(&self, callback: F) -> Result<&Self, WS::Error>;
    fn on_destroy<F: FnMut() + 'static>(&self, callback: F) -> Result<&Self, WS::Error>;
}
