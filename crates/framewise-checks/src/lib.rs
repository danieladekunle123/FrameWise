//! FrameWise check definitions, shared by every FrameWise process.
//!
//! The crate is split in two so detection logic is testable without real hardware:
//!
//! * [`collect`] (Windows only) reads the system into a serialisable [`Snapshot`]. It never
//!   changes anything.
//! * [`evaluate`] turns a snapshot into [`Finding`]s with pure functions. Unit tests feed it
//!   recorded snapshots.
//!
//! [`score`] turns findings into the 0–100 health score.
//!
//! From phase 2, [`tweak`] defines the settings FrameWise can change, [`fixes`] which checks can
//! be fixed and how, [`system`] (Windows) reads and writes them, and [`cleanup`] measures and
//! removes junk files.

pub mod cleanup;
pub mod crash;
pub mod evaluate;
pub mod fixes;
pub mod frames;
pub mod monitor;
pub mod profiles;
pub mod model;
pub mod myths;
pub mod parse;
pub mod score;
pub mod snapshot;
pub mod tweak;

#[cfg(windows)]
pub mod collect;
#[cfg(windows)]
pub mod procmeter;
#[cfg(windows)]
pub mod library;
#[cfg(windows)]
pub mod sensors;
#[cfg(windows)]
pub mod system;

pub use evaluate::{evaluate_all, evaluate_one, CHECK_IDS};
pub use model::*;
pub use score::health_score;
pub use snapshot::Snapshot;
