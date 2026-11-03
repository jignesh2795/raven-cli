# Security — Raven CLI

This document states what is true **now**, not what is planned. Capabilities are
versioned; read the status line before relying on a guarantee.

## Reporting a vulnerability

Do not open a public issue for a vulnerability. Contact the maintainer privately
and include the version, Windows build, reproduction steps, and impact. Do not
include real credentials. No public disclosure before a fix is available.

## Status

- **Version:** 0.2.0 (M1)
- **Execution mode:** file writes under the `workspace-write` policy. Every edit
  is exact search/replace, atomic, hash-verified, and shown to the human for
  approval before it is written. No command execution, no agent-initiated child
  processes, no network tools. `doctor` runs `git --version` as a version probe.
- **OS sandbox:** not implemented. `raven doctor` reports MXC/AppContainer as
  *planned*, not available.
- **Guarantees below are scoped to M1.** The M3 sandbox guarantees are not in
  effect yet.

## Security modes

| Mode | Status | What it means |
| --- | --- | --- |
| read-only | **Available** | Opt in with `--read-only` or `policy = "read-only"`. Only read/list/search tools; no write tool is even registered. |
| workspace-write (M1) | **Default** | Read plus exact search/replace edits, each previewed as a diff and approved before it is written. Policy is in-process. |
| best-effort (M2) | Planned | Policy + audit around command execution. **Not a security boundary.** |
| `mxc` (M3) | Planned | Microsoft Execution Containers. Preferred OS boundary where supported. |
| `appcontainer` (M3) | Planned | AppContainer + Job Object + restricted token fallback. OS boundary with a compatibility cost. |
| `off` | Planned | No containment. Explicit opt-in only; never the default. |
| `vm` | Planned (optional) | VM tier for untrusted repositories. Not implemented. |

A weaker mode must never be displayed as equivalent to a stronger one. `doctor`
reports the actual backend and its limits.

## M1 guarantees (implemented)

1. **Writes are human-authorized.** No file change happens without a decision at
   the approval prompt (or an explicit `--yes`). A non-interactive run without
   `--yes` applies nothing and reports the denial instead. The CLI's approver is
   always installed; the agent's built-in default is `DenyAll`, so a missing
   wiring denies rather than writes silently.
2. **No execution capabilities.** `edit_file` and `create_file` are the only
   write tools. The policy rejects execute and network tool kinds. The agent loop
   starts no child processes; `doctor` runs only `git --version`.
3. **Atomic, verified writes.** Each edit is written to a temporary file, synced,
   verified byte-for-byte, then swapped in with `ReplaceFileW` (existing target)
   or `MoveFileExW` with `MOVEFILE_WRITE_THROUGH` (new target). There is no
   delete-then-rename fallback: if the swap cannot be atomic, it fails and the
   original is untouched. The pre-image hash is re-checked between approval and
   the swap, and the result is re-read and hash-compared afterwards.
4. **Exact edits only.** `edit_file` replaces exact text that must occur exactly
   once unless `replace_all` is set; there are no line-number patches and no
   whole-file rewrites. The UTF-8 BOM and the file's dominant line-ending style
   are preserved. `create_file` never overwrites an existing path.
5. **Workspace confinement on writes.** Write paths resolve the parent through
   canonicalization (so symlinks and junctions are followed and checked), reject
   protected names, and normalize `.env::$DATA`, trailing-dot, and trailing-space
   bypasses. Reserved Windows device names and alternate-data-stream colons are
   refused.
6. **Audit of every change.** Each applied edit appends an `edit_applied` event
   carrying the path, before/after SHA-256 hashes, added/removed counts, and the
   redacted diff.
2. **Workspace confinement.** Tool paths are canonicalized and confined to the
   workspace root. Traversal, device paths, named pipes, `.git` components, and
   secret-bearing filenames (`.env*`, private keys, `.npmrc`, `.pypirc`,
   credential stores) are denied.
3. **No project-controlled configuration.** Only the user config
   (`%APPDATA%\raven\raven.toml`, `--config`, or `RAVEN_CONFIG`) is loaded. A
   cloned repository cannot choose the endpoint or key source.
4. **Credential handling.** `raven auth set` stores keys in Windows Credential
   Manager (service `raven-cli`). Keys are not written to `raven.toml` or the
   event log.
5. **Redaction before egress and persistence.** Common API keys, JWTs, bearer
   tokens, private keys, and URL credentials are redacted before tool results
   are sent to the provider, written to the event log, rendered, or shown in an
   approval diff. This is pattern-based and best-effort. `redact = false` turns
   this off entirely; it is a debugging escape hatch, not a supported
   production setting.
6. **Terminal safety.** ANSI CSI/OSC and other control characters are stripped
   from file content, tool output, and streamed model text before it reaches the
   terminal or JSON output.
7. **Output limits.** Reads are capped at 4 MiB; tool output defaults to 64 KiB;
   directory listings cap at 1000 entries; search caps at 500 results. Files
   with NUL bytes are rejected as binary.
8. **Tamper-evident audit.** Each session event is SHA-256 hash-chained; `show`
   verifies sequence, links, and hashes. The log is tamper-evident, not
   tamper-proof.
9. **Fail-closed limits.** Turn limits stop the loop, and cost limits stop the
   session after the response that pushes the total over the cap. `--max-cost`
   refuses to run without a matching pricing entry instead of guessing.
10. **No daemon.** The CLI opens no listening socket. Provider traffic is the
    only outbound network path.

## M1 non-guarantees (explicitly not claimed)

- **No OS containment boundary.** Path policy and redaction live in the same
  process as the agent. A compromised binary or dependency can bypass both.
- **No write or network isolation at the OS level.** Edits are confined by the
  in-process path policy, which a compromised process could ignore; containment
  arrives with M3.
- **Redaction is not complete.** Unknown secret formats, high-entropy strings
  without a recognizable prefix, and secrets already present in the user's
  prompt are not reliably caught. `crates/raven-cli/tests/canary.rs` plants
  known formats in a workspace and proves none reach a provider payload, the
  audit log, or an approval diff; it is a regression test, not a proof.
- **No hardlink protection.** A hard link created inside the workspace can
  reference content outside it. Detecting hardlinks requires OS-level checks and
  belongs with the M3 sandbox.
- **No crash-dump or memory protection.** Keys exist in process memory and HTTP
  headers. Memory zeroization, dump exclusion, and provider-side retention are
  out of scope for M1.
- **Tamper-evident, not tamper-proof.** An attacker with write access to the
  session directory can rewrite the entire chain.
- **Endpoint trust is the user's choice.** Content sent to a configured provider
  is subject to that provider's policy. TLS certificate validation uses the
  system trust store; there is no certificate pinning.
- **Prompt injection is mitigated, not solved.** Repository content is labeled
  untrusted and cannot grant capabilities, but a model can still be induced to
  read files the policy allows, or to propose an edit it presents as useful.
  The approval prompt is the last barrier, not a detector. Data disclosure and
  bad edits the user approves are both possible.
- **Supply chain is not attested.** Dependencies are version-locked by
  `Cargo.lock`, but there is no reproducible-build attestation or signed release
  yet.

## Planned guarantees by milestone

### M2 — best-effort command containment

- Job Object with `KILL_ON_JOB_CLOSE`; parent death leaves no descendants.
- Sanitized environment; no inherited secrets unless explicitly allowed.
- Working-directory confinement, timeout, output caps.
- Clearly labeled **not a security boundary**.

### M3 — OS sandbox (MXC / AppContainer)

- No write outside granted roots, tested against symlink, junction, hardlink,
  ADS, UNC, 8.3, and device-path tricks.
- Offline policy blocks socket, DNS, and ICMP canaries.
- Parent termination leaves no descendants.
- Elevated execution refused.
- Per-backend compatibility profiles and an escape suite.
- `doctor` reports the real backend and tested Windows build numbers.
- Guarantees are per backend; `unelevated`/fallback isolation is not described
  as equivalent to MXC.

### M4 — checkpoints

- `commit-tree` + temporary index/refs; the user's HEAD, index, and working tree
  are untouched by checkpoint creation.
- Fault injection at every step.

### M5 — MCP

- Explicit allowlist, workspace trust gate, hash-pinned servers/config.
- Per-server environment allowlist and secret scrubbing.
- Sandboxed server processes.
- Tool descriptions/results treated as untrusted input.

### M6 — headless/CI

- Fail-closed policy without a TTY.
- Versioned JSONL, stable exit codes, enforceable cost caps.

### M8 — plugins

- Out-of-process or WASM only; permission manifests; signing; crash isolation.

## Secret handling

- **At rest:** Windows Credential Manager, service `raven-cli`. Environment
  variables are supported for CI; they are not persisted by Raven.
- **In transit:** bearer token to the configured base URL over TLS. Localhost
  endpoints may be plain HTTP by design.
- **At read time:** denylist for common secret files; pattern redaction before
  tool-result provider payloads, the event log, and the UI.
- **In logs:** event summaries and data are redacted before serialization.
  Session IDs are validated to prevent path traversal.
- **In errors:** provider error bodies are truncated to 1200 characters before
  display; this is not a redaction guarantee for arbitrary provider text.

## Build and distribution (planned hardening)

- Release builds with `--locked`; commit `Cargo.lock`.
- Reduce default features and review dependency updates.
- Add `cargo audit`/`cargo deny` to CI.
- Code-sign the binary and publish hashes before recommending winget/scoop.
- Do not auto-install plugins, skills, or MCP servers.

## Disclosure

Report vulnerabilities privately. No public disclosure before a fix. This
document will be updated when guarantees change; an unqualified claim in an old
release note does not override the versioned status here.
