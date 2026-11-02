use crate::{ProbeResult, SandboxProbe};

/// Probe sandbox support without changing system state.
///
/// M0 is deliberately conservative: it does not claim that a containment
/// boundary exists until the backend is implemented and its escape suite is
/// green. Codex-style native Windows sandboxing is table stakes in 2026, not a
/// differentiator on its own; see `docs/codex-windows-status.md`.
pub fn probe() -> SandboxProbe {
    SandboxProbe {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        mxc: ProbeResult::Planned(
            "MXC detection and command execution land in M3".into(),
        ),
        appcontainer: ProbeResult::Planned(
            "AppContainer + Job Object backend lands in M3".into(),
        ),
        best_effort: ProbeResult::Planned(
            "command execution and sanitized environment land in M2".into(),
        ),
        vm: ProbeResult::Unavailable(
            "optional VM tier is not implemented in v0.1".into(),
        ),
    }
}
