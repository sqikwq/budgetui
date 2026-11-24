use ratatui::style::Color;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Palette {
    pub(crate) background: Color,
    pub(crate) surface: Color,
    pub(crate) text: Color,
    pub(crate) muted: Color,
    pub(crate) accent: Color,
    pub(crate) positive: Color,
    pub(crate) negative: Color,
    pub(crate) contrast: Color,
}

impl Palette {
    pub(crate) const fn classic() -> Self {
        Self {
            background: Color::Reset,
            surface: Color::Rgb(28, 43, 64),
            text: Color::White,
            muted: Color::DarkGray,
            accent: Color::Cyan,
            positive: Color::Green,
            negative: Color::LightRed,
            contrast: Color::Black,
        }
    }
}
