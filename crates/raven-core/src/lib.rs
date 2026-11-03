#![deny(unsafe_code)]

//! Provider-agnostic agent core.
//!
//! The core owns the tool loop, the event model, policy checks, secret
//! redaction, and cost accounting. It deliberately does not know how to talk
//! to a specific model provider or how to execute commands on Windows; those
//! live in dedicated crates.

pub mod agent;
pub mod cost;
pub mod error;
pub mod events;
pub mod message;
pub mod policy;
pub mod provider;
pub mod redact;
pub mod tool;
pub mod util;

pub use error::{Error, Result};
