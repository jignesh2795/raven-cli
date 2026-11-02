#![deny(unsafe_code)]

//! Windows execution sandbox.
//!
//! M0 only probes and documents available modes; it does not execute commands.
//! All Windows FFI for containment will live in this crate (and only this
//! crate): MXC where available, then AppContainer + Job Object + restricted
//! token, then an explicitly labelled best-effort mode.

pub mod windows;

pub use windows::probe;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxMode {
    /// No containment. Explicit opt-in only.
    Off,
    /// Policy and audit only. NOT a security boundary.
    BestEffort,
    /// Microsoft Execution Containers, when the OS build and policy support it.
    Mxc,
    /// AppContainer + Job Object + restricted token fallback.
    AppContainer,
    /// Optional VM tier for untrusted repositories.
    Vm,
}

impl SandboxMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::BestEffort => "best-effort",
            Self::Mxc => "mxc",
            Self::AppContainer => "appcontainer",
            Self::Vm => "vm",
        }
    }

    pub fn is_boundary(self) -> bool {
        matches!(self, Self::Mxc | Self::AppContainer | Self::Vm)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status", content = "detail")]
pub enum ProbeResult {
    Available,
    Unavailable(String),
    Unknown(String),
    Planned(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxProbe {
    pub os: String,
    pub arch: String,
    pub mxc: ProbeResult,
    pub appcontainer: ProbeResult,
    pub best_effort: ProbeResult,
    pub vm: ProbeResult,
}
