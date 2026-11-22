pub mod config;
mod handler;
pub mod layout;
mod ui;

#[cfg(test)]
#[path = "tests/mod_tests.rs"]
mod tests;

pub use config::INPUT_MIN_WIDTH;
pub use handler::focus_text_field_under_pointer;
pub use layout::resolve_field_width;
pub use ui::{Input, PasswordInput, SearchInput, Textarea};
