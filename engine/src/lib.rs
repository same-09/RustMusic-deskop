//! RustMusic FLAC engine.
//!
//! Plays a FLAC file at its own sample rate and bit depth, starting while the file is still
//! downloading. Rust port of the old Kotlin `FlacEngine`.
//!
//! The device side is the [`PcmOut`] trait; [`CpalOut`] (feature `cpal-out`) is the real one.

mod convert;
mod flac;
mod out;
#[cfg(feature = "cpal-out")]
mod cpal_out;

pub use flac::{Callbacks, Engine, FlacEngine, Source};
pub use out::PcmOut;
#[cfg(feature = "cpal-out")]
pub use cpal_out::CpalOut;
