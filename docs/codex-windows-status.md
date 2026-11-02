# OpenAI Codex on Windows — verified status

**Verified:** 2026-11-02
**Source:** <https://developers.openai.com/codex/windows/windows-sandbox>

This note exists because the original Raven CLI plan assumed that
"Codex-grade containment on Windows" was an open gap. It is not. Codex now
ships native Windows sandboxing, with three backends. Raven must not claim
that native Windows containment is a differentiator.

## What Codex ships

| Backend | Status | Requirements | Isolation character |
| --- | --- | --- | --- |
| `mxc` | Recommended default where supported | Windows 11 24H2 build 26100.9278+ or 25H2 build 26200.9278+ | Microsoft Execution Containers; no admin setup |
| `elevated` | Legacy fallback | Administrator-approved setup; dedicated sandbox users | Separate user accounts, filesystem ACLs, firewall rules |
| `unelevated` | Fallback when policy permits | No admin setup | Restricted token, ACLs, environment offline; weaker network isolation |

Additional details from the documentation:

- Codex runs natively from PowerShell or Windows Terminal; WSL 2 is optional,
  not required.
- Windows 10 is best-effort and past end of support; the modern backends target
  Windows 11.
- The sandbox is selected and configured by Codex itself. The user can inspect
  and override the backend.
- `mxc` is the default on compatible devices. The other modes exist for
  compatibility, not because they are preferred.

OpenAI also publishes a Windows sandbox setup command for the elevated mode
(`codex sandbox setup --elevated --current-user`) and documents that some tools
need to run outside the sandbox. The exact flags and defaults may change; treat
the official page as authoritative.

## What this invalidates

The original positioning — "the wedge is Codex-grade containment on Windows" —
is no longer defensible:

1. **Parity is not differentiation.** A new CLI that implements AppContainer +
   Job Objects would match a shipped feature, not create an advantage.
2. **The hard part is compatibility, not the syscall sequence.** Codex's
   backends exist because real toolchains (npm, cargo, Python, git) break under
   isolation. Shipping an escape-suite-green sandbox that cannot run a normal
   build is not a usable product.
3. **"Windows-first" alone is weak.** Codex, Cline, and other agents already run
   natively on Windows. The remaining gap is not the operating system.
4. **A weaker fallback must be labeled honestly.** `unelevated` mode is not
   equivalent to `mxc`; the same is true for any Raven fallback. Marketing
   must not flatten the difference.

## Revised differentiation

Raven should compete on properties that do not depend on out-sandboxing
OpenAI:

- **Provider-neutral by construction.** One OpenAI-compatible adapter covers
  OpenAI, NVIDIA NIM, Ollama, and self-hosted gateways. Keys stay in Windows
  Credential Manager; local endpoints need no key. No vendor account is
  required to use the tool.
- **Local-first and offline-capable.** Ollama and local gateways work without a
  cloud dependency. Network access for tools is off in M0 and will be an
  explicit, auditable capability later.
- **Auditable, open security.** A local hash-chained event log, a documented
  threat model, and reproducible tests. Users can verify what the agent did
  without trusting a vendor dashboard.
- **Enterprise policy without a control plane.** Fail-closed policy,
  user-owned configuration, no daemon, no telemetry, and no hidden network
  calls. A team can audit and pin the binary it runs.
- **Honest capability boundaries.** "Read-only" means read-only. "Not a
  boundary" is written on the tin. Sandbox modes are reported by `doctor` and
  documented per backend.
- **Windows ergonomics.** Native paths, Credential Manager, a single binary,
  and no WSL requirement. This is table stakes, but doing it well still
  matters.

## Consequences for the roadmap

- **M3 must implement two backends, not one.** Prefer MXC where the OS build
  and policy support it; fall back to AppContainer + Job Object + restricted
  token. Each backend gets its own compatibility profile and escape suite.
- **`doctor` must report the actual backend and its guarantees**, not a boolean
  "sandbox: on".
- **M0 must not pretend to be sandboxed.** It is a read-only tool loop with no
  command execution. Path policy and redaction are mitigations, not an OS
  boundary.
- **Do not use "Codex-grade" in product copy.** Compare on provider freedom,
  auditability, and local operation, and link to this document when the sandbox
  question comes up.

## Open questions to re-check before M3

- Whether MXC is callable by a non-Codex process on current Windows builds, and
  under what license/API surface.
- Minimum build numbers and SKU requirements for MXC on Windows 11.
- Whether AppContainer + Job Objects can cover the same toolchain compatibility
  set without the dedicated-user approach.
- Codex's current defaults on Windows 10 and whether `mxc` ever silently falls
  back.

Re-verify against the official page before committing to a backend design.
