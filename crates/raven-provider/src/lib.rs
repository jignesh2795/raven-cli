#![deny(unsafe_code)]

//! OpenAI-compatible provider adapter.
//!
//! The same wire format covers OpenAI, NVIDIA NIM, Ollama's `/v1` endpoint, and
//! most self-hosted gateways. Capability differences are handled by
//! configuration and by accepting partial/streamed tool-call payloads.

mod cassette;
mod openai;

pub use cassette::{Cassette, CassetteProvider, Recorder};
pub use openai::OpenAiProvider;
