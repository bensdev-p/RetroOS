//! `retro-sys`: everything RetroOS front-ends do to the system goes through
//! here, so the text and GUI versions of a tool cannot drift apart
//! (docs/DESIGN.md §10).

pub mod admin;
pub mod clock;
pub mod display;
pub mod keyboard;
pub mod net;
pub mod netconf;
pub mod startup;
pub mod users;
