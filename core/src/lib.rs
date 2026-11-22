//! NoopFlow core: share one keyboard and mouse between Windows and Mac computers.

pub mod clipboard;
pub mod config;
pub mod engine;
pub mod files;
pub mod keys;
pub mod net;
pub mod platform;
pub mod protocol;

pub use config::Settings;
pub use engine::{start, Cmd, CoreEvent, CoreHandle, Snapshot};
pub use protocol::Cell;
