# Milestones — Raven CLI

Status as of **2026-11-03**, version **0.2.0**. The plan is deliberately
incremental: prove the provider loop and honest accounting before adding
capabilities that need containment.

## M0 — Provider + streaming + read-only tool loop ✅

**Scope**

- Rust workspace with a one-way dependency graph and `#![deny(unsafe_code)]`
  everywhere.
- OpenAI-compatible SSE provider adapter covering OpenAI, NVIDIA NIM, Ollama,
  and custom endpoints.
- Three read-only tools: `read_file`, `list_dir`, `search_files`.
- Workspace path policy, protected-path denylist, secret redaction, ANSI/OSC
  stripping, output caps.
- Ctrl+C cancellation, max-turn limit, token accounting, optional cost cap.
- Append-only hash-chained JSONL session log plus `meta.json`.
- CLI: `run`, `init`, `auth`, `doctor`, `sessions`, `show`; human output and
  versioned JSONL.
- Local mock provider and PowerShell end-to-end test.

**Exit evidence**

| Criterion | Evidence |
| --- | --- |
| Builds on Windows | `cargo build` succeeds with MSVC 14.29 + Windows SDK libraries |
| Unit tests | `cargo test --workspace`: 17 passed (10 core, 4 provider, 2 store, 1 tools) |
| Streaming + tool loop | `scripts/e2e.ps1`: 2 turns, `read_file` call, final answer |
| Accounting | E2E reports 300 input / 42 output tokens and `$0.0004` with test pricing |
| Cancellation | `cancellation_stops_an_in_flight_stream` fake-provider test; checkpoints before turns, between stream items, and before tools; exit code 130 |
| Audit log | `raven show` verifies sequence, `prev_hash`, and per-event SHA-256 |
| Redaction | Unit tests cover API keys, JWTs, bearer tokens, private keys, JSON payloads, and log writes |
| Path policy | Unit tests cover traversal escape, `.env`, key files, and `.git` |

**Known gaps after M0**

- Provider cassette **record/replay** is not implemented. The deterministic
  mock provider (`scripts/mock_provider.py`) covers the end-to-end path, but a
  recorded real-provider fixture is deferred to M1 alongside the SQLite
  projection.
- Cancellation is covered by a fake-provider unit test, not yet by a large real-
  provider stream and a slow tool. Adversarial cancellation tests belong in
  M1/M2.
- No Windows OS sandbox. The agent loop executes no child processes (`doctor`
  runs `git --version`), so there is nothing to contain yet; path policy and
  redaction are in-process mitigations.

## M1 — Safe edits + policy + audit hardening

- Exact `old`/`new` search-replace with uniqueness checks, atomic replace, and
  post-write hash verification; preserve EOL/BOM.
- Workspace-write policy with re-checks at write time (TOCTOU-aware).
- SQLite projection over the event log; rebuildable from JSONL.
- Secret canary suite proving canaries never reach provider payloads, logs, or
  error output; entropy detection where practical. **Canary suite done** (see
  below); entropy detection still M1.
- Provider record/replay cassettes for deterministic tests.
- Workspace trust gate for project-local configuration.

**Exit:** atomic edits preserve EOL/BOM; path-policy tests cover symlink,
junction, and protected-path cases; canaries never leave; replay fixtures are
stable across runs.

**Progress (2026-11-03):** edit + create + approval + audit hardening landed
(commit `c249bbd`); secret canary suite landed in the next commit. Remaining M1:
SQLite projection, provider record/replay cassettes, workspace trust gate,
entropy detection.

## M2 — Command runner + best-effort containment

- Command execution with explicit approval modes.
- Job Object with `KILL_ON_JOB_CLOSE`; sanitized environment; working-directory
  confinement; output caps and timeout.
- Best-effort mode: policy + audit, **not a security boundary**.
- ConPTY support where a real terminal is required.

**Exit:** killing the parent leaves no descendants; environment cannot leak
unapproved variables; output caps hold under a hostile child; the mode is
reported as best-effort everywhere.

## M3 — OS sandbox: MXC + AppContainer fallback

Codex ships native Windows sandboxing, so this milestone is parity work, not a
differentiator. See `docs/codex-windows-status.md`.

- MXC backend when the Windows build and policy support it.
- AppContainer + Job Object + restricted token fallback.
- Per-backend compatibility profiles for npm, cargo, Python, and git.
- Escape suite: filesystem, network, process, environment, path aliasing,
  symlink/junction/hardlink, ADS, UNC, 8.3 names, device paths.
- Refuse elevation. Report backend and guarantees in `doctor`; never present a
  fallback as equivalent to the stronger backend.

**Exit:** escape suite green on both backends; toolchain smoke tests pass; no
write outside granted roots; offline policy blocks socket/DNS/ICMP canaries;
`doctor` reports actual guarantees; guarantees document updated with tested
build numbers.

## M4 — Checkpoints and undo

- Checkpoints via `git commit-tree` + a temporary index and temporary refs.
- Never mutate the user's HEAD, index, or working tree during checkpointing.
- Fault injection proves HEAD/index untouched.
- Non-git snapshot fallback and explicit restore.

**Exit:** failure injected at every step leaves HEAD/index unchanged; restore
works from both git and snapshot checkpoints.

## M5 — MCP

- Allowlist, plus the M1 workspace trust gate extended to MCP server
  configuration before any project-level definition is honored.
- Hash-pinned server executables and configuration.
- Per-server environment allowlist and secret scrubbing.
- Sandboxed server child processes.
- Prompt-injection tests for tool descriptions and results.

**Exit:** unsigned/changed servers refuse to start; env scrubbing tests pass;
injection fixtures cannot grant capabilities.

## M6 — Headless/CI hardening

- Fail-closed policy in non-interactive mode.
- Versioned JSONL contract, stable exit codes, cost caps.
- No TTY assumptions; deterministic diagnostics on stderr.

**Exit:** CI matrix passes with no TTY; budget/policy failures are
distinguishable; schema version is documented and tested.

## M7 — Skills

- Loadable prompt modules with precedence rules and provenance.
- No auto-install and no execution from a repository without trust.
- Skill content labeled untrusted; cannot grant capabilities.

**Exit:** precedence tests pass; provenance is recorded; a malicious skill
cannot change policy.

## M8 — Plugins

- Out-of-process or WASM isolation; no in-process native plugins.
- Permission manifests, signing, crash isolation.
- No silent network or filesystem access.

**Exit:** unsigned plugins refuse to load; crash isolation verified; permission
violations are audited and denied.

## Verification log

| Date | Check | Result |
| --- | --- | --- |
| 2026-11-02 | `cargo build` (debug, Windows MSVC) | Pass |
| 2026-11-02 | `cargo build --release` (Windows MSVC) | Pass |
| 2026-11-02 | `cargo test --workspace` | 17 passed |
| 2026-11-02 | `scripts/e2e.ps1` (release binary) | Pass, 2 turns, 342 tokens, `$0.0004` |
| 2026-11-02 | `raven doctor` | Config/data/workspace/providers/sandbox reported |
| 2026-11-02 | Codex Windows sandbox re-check | Native backends exist; positioning revised |
| 2026-11-03 | `cargo fmt --all -- --check` | Pass (rustfmt 1.99.1) |
| 2026-11-03 | `cargo clippy --workspace --all-targets -- -D warnings` | Clean (clippy 1.99.1) |
| 2026-11-03 | `cargo test --workspace` after formatting | 17 passed |
| 2026-11-03 | `scripts/e2e.ps1` after formatting | Pass |
| 2026-11-03 | `raven-cli` M1 edit + create + approval + audit | Builds clean, 32 tests, E2E passes |
| 2026-11-03 | `raven doctor` (post-M1) | Reports workspace-write policy |
| 2026-11-03 | Windows SDK persistence fix | `-DownloadSdk` writes `%LOCALAPPDATA%\raven-sdk`; plain `dev-env.ps1` auto-detects it; build/tests/clippy/fmt green in a clean shell with no flags |

Root cause of the earlier "no Windows SDK" report: the SDK **was** installed
(the successful builds in this session used a NuGet-extracted SDK), but
`dev-env.ps1` only auto-detected SDKs from `Windows Kits\10\Lib`. The persisted
NuGet SDK at `%LOCALAPPDATA%\raven-sdk\sdk-*` was invisible unless `-SdkPath`
was passed explicitly, so a new shell looked like a machine with no SDK. Two
script fixes:

- `Find-PersistedNuGetSdk` scans `%LOCALAPPDATA%\raven-sdk\sdk-*` and is tried
  after an installed SDK, before `-DownloadSdk`.
- `Expand-Archive` rejects the `.nupkg` extension (zip only), which crashed the
  download path; extraction now uses `ZipFile.ExtractToDirectory`, which
  inspects the archive content rather than the file name.

Notes on M1 scope delivered in this commit:
- `PolicyMode::WorkspaceWrite` + `WorkspaceWrite` registry with `edit_file` and `create_file`.
- `ToolContext.approver` wired through `Agent::with_approver()`; default `DenyAll`.
- `ConsoleApprover` in `raven-cli/src/approve.rs`: diff preview on stderr, `y/n/a` prompt, `--yes` skips prompt.
- `Policy::resolve_for_write()` (new file allowed, protected paths blocked, symlink/junction targets resolved before policy check, reserved Windows names and ADS denied).
- `PathPolicy.normalize_name()` handles `.env::$DATA` bypasses.
- `atomic.rs` in `raven-sandbox`: `ReplaceFileW` (existing) + `MoveFileExW` with `MOVEFILE_WRITE_THROUGH` (new), with `TempFile` guard.
- `write_support` module: BOM/EOL preservation, `detect_eol()`, `encode()`, `sha256_hex()`.
- `edit_file`: exact `old`/`new`, uniqueness check or `replace_all`, post-edit hash verify, `EditApplied` event with before/after SHA-256.
- `create_file`: only creates missing files, never overwrites.
- `events.jsonl` records `edit_applied` events with hashes.
- `prompt.rs` updated: edit rules added, `PolicyMode` passed through.
- `e2e.ps1` now exercises read → edit → create → audit.
- `sample_toml()` and `Config` include `policy = "workspace-write"`.
