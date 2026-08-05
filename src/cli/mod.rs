//! Hypha's closed-world command-line boundary.

mod resolve;
mod run;
mod spec;
mod types;

pub use resolve::parse_or_exit;
pub use run::execute;
pub use types::*;
