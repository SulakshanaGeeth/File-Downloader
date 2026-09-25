//! Flow's UI-independent download engine.
//!
//! Create a [`Manager`], enqueue URLs with [`Manager::add`], and call
//! [`Manager::start`] inside a Tokio runtime. Poll [`Manager::snapshot`] for
//! progress and always await [`Manager::shutdown`] before exiting.

mod manager;
mod model;
mod paths;
mod store;
mod transfer;

pub use anyhow::Result;
pub use manager::Manager;
pub use model::{Action, AddRequest, Direction, Download, Settings, Snapshot, Status, Theme};
