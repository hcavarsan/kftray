use ratatui::prelude::Color;

pub(crate) mod draw;
mod header;
pub(crate) mod popup;
pub(crate) mod render;
pub(crate) mod table;
pub(crate) use draw::*;
pub(crate) use popup::*;
pub(crate) use render::*;
pub(crate) use table::*;

pub(crate) const PINK: Color = Color::Rgb(245, 194, 231);
pub(crate) const MAUVE: Color = Color::Rgb(203, 166, 247);
pub(crate) const RED: Color = Color::Rgb(243, 139, 168);

pub(crate) const YELLOW: Color = Color::Rgb(249, 226, 175);
pub(crate) const GREEN: Color = Color::Rgb(166, 227, 161);
pub(crate) const TEAL: Color = Color::Rgb(148, 226, 213);

pub(crate) const LAVENDER: Color = Color::Rgb(180, 190, 254);
pub(crate) const TEXT: Color = Color::Rgb(205, 214, 244);

pub(crate) const SURFACE2: Color = Color::Rgb(88, 91, 112);
pub(crate) const SURFACE1: Color = Color::Rgb(69, 71, 90);
pub(crate) const SURFACE0: Color = Color::Rgb(49, 50, 68);
pub(crate) const BASE: Color = Color::Rgb(30, 30, 46);
pub(crate) const MANTLE: Color = Color::Rgb(24, 24, 37);
pub(crate) const CRUST: Color = Color::Rgb(17, 17, 27);

pub(crate) const SUBTEXT1: Color = Color::Rgb(186, 194, 222);
pub(crate) const SUBTEXT0: Color = Color::Rgb(166, 173, 200);
//pub const OVERLAY2: Color = Color::Rgb(147, 153, 178);
//pub const OVERLAY1: Color = Color::Rgb(127, 132, 156);
//pub const OVERLAY0: Color = Color::Rgb(108, 112, 134);
//pub const SKY: Color = Color::Rgb(137, 220, 235);
//pub const SAPPHIRE: Color = Color::Rgb(116, 199, 236);
pub(crate) const BLUE: Color = Color::Rgb(137, 180, 250);
//pub const MAROON: Color = Color::Rgb(235, 160, 172);
//pub const PEACH: Color = Color::Rgb(250, 179, 135);
//pub const ROSEWATER: Color = Color::Rgb(245, 224, 220);
//pub const FLAMINGO: Color = Color::Rgb(242, 205, 205);
