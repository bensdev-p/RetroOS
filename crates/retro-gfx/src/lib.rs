//! retro-gfx: the drawing layer shared by every RetroOS screen.
//!
//! Everything RetroOS draws itself — the POST screen, the greeter, the text
//! console, Station's chrome and the retro-ui toolkit — goes through these
//! types: an XRGB8888 [`Surface`], the [`palette`], and bitmap [`font`]s.

pub mod font;
pub mod grid;
pub mod palette;
mod surface;

pub use font::{Font, Glyph};
pub use grid::{Cell, Grid};
pub use palette::{Rgb, Scheme};
pub use surface::{Canvas, Surface};
