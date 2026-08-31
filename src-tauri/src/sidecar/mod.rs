pub mod adapter;
mod runner;

pub use adapter::{normalize_reports, normalize_snapshot};
pub use runner::{begin_shutdown, collect_usage, is_shutting_down};
