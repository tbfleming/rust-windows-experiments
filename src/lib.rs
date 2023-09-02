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

pub trait Window<'event>: Sized + Clone {
    type Error: std::error::Error;
    type Child: Window<'event>;

    fn destroy(&self) -> Result<(), Self::Error>;
    fn create_child(&self, ty: ChildType) -> Result<Self::Child, Self::Error>;

    fn text(self, text: &str) -> Result<Self, Self::Error>;
    fn bounds(
        self,
        upper_left: Option<(i32, i32)>,
        size: Option<(i32, i32)>,
    ) -> Result<Self, Self::Error>;
    fn background(self, color: Color) -> Result<Self, Self::Error>;
    fn visible(self, visible: bool) -> Result<Self, Self::Error>;
    fn redraw(self) -> Result<Self, Self::Error>;

    fn on_close<F: FnMut(&Self) + 'event>(self, callback: F) -> Result<Self, Self::Error>;
    fn on_destroy<F: FnMut(&Self) + 'event>(self, callback: F) -> Result<Self, Self::Error>;
}
