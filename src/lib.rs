pub mod comm_ctrl;

pub struct Color(u8, u8, u8, u8);

pub trait Window<'event>: Sized + Clone {
    type Error: std::error::Error;
    type Child: Window<'event>;

    fn destroy(&self) -> Result<(), Self::Error>;
    fn create_child(&self) -> Result<&Self::Child, Self::Error>;

    fn text(&self, text: &str) -> Result<&Self, Self::Error>;
    fn bounds(
        &self,
        upper_left: Option<(i32, i32)>,
        size: Option<(i32, i32)>,
    ) -> Result<&Self, Self::Error>;
    fn background(&self, color: Color) -> Result<&Self, Self::Error>;
    fn show(&self, visible: bool) -> Result<&Self, Self::Error>;
    fn enable(&self, enabled: bool) -> Result<&Self, Self::Error>;

    fn on_close<F: FnMut(&Self) + 'event>(&self, callback: Option<F>)
        -> Result<&Self, Self::Error>;
}
