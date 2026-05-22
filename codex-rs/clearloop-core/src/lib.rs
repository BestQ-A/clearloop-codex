//! ClearLoop cognitive core primitives for visible programmatic thinking.
//!
//! This crate intentionally stays independent from Codex orchestration crates.
//! Codex surfaces can depend on it later without pushing ClearLoop concepts into
//! `codex-core`.

mod domain;
mod error;
mod event_bridge;
mod ledger;
mod maturity;
mod store;

pub use domain::*;
pub use error::ClearLoopError;
pub use error::Result;
pub use event_bridge::*;
pub use ledger::*;
pub use maturity::*;
pub use store::ClearLoopStore;

pub const SCHEMA_VERSION: &str = "codex-clearloop-core.v0";
