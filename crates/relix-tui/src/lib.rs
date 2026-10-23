/*
 * @author t0bx
 * @since 23.10.2026
 */

pub mod clipboard;
pub mod colors;
pub mod event_loop;
pub mod input_map;
pub mod terminal;
pub mod text;
pub mod ui;

pub use clipboard::Osc52Clipboard;
pub use event_loop::run;
