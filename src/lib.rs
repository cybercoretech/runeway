//! Runeway library: event model, sources and world state. No terminal code.

pub mod collector;
pub mod config;
pub mod demo;
pub mod error;
pub mod model;
pub mod sources;
pub mod status;
pub mod world;

pub use error::{Error, Result};
