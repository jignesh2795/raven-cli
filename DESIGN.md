# Design — Raven CLI

## Goals

A Windows-native coding agent that is provider-neutral, auditable, and honest
about its boundaries. Single binary, no daemon in v1. Security-critical Windows
code is isolated in one crate.

## Architecture

```text
CLI (raven-cli)
  │  config, auth, rendering, command dispatch
  ├── raven-core
  │     agent loop · events · provider/tool traits · policy · redaction · cost
  ├── raven-provider
  │     OpenAI-compatible SSE adapter (OpenAI, NIM, Ollama, custom)
  ├── raven-tools
  │     read-only tools for M0
  ├── raven-store
  │     append-only session log + metadata
  └── raven-sandbox
        probe now; Windows containment FFI in M2/M3
```

`raven-core` is deliberately ignorant of HTTP, the filesystem tool
implementations, storage, and Windows APIs. Providers, tools, and stores plug in
behind traits.

### Current crates

| Crate | Responsibility | Unsafe |
| --- | --- | --- |
| `raven-core` | Agent loop, messages, provider/tool traits, policy, redaction, cost, event model | Denied |
| `raven-provider` | One OpenAI-compatible streaming adapter with retries and SSE tool-call assembly | Denied |
| `raven-tools` | `read_file`, `list_dir`, `search_files` | Denied |
| `raven-store` | JSONL hash-chained event log, `meta.json`, verification, session listing | Denied |
| `raven-sandbox` | Atomic file replace; sandbox probe; future MXC/AppContainer/Job Object code | Sole `unsafe` exception; `atomic.rs` is the current user |
| `raven-cli` | `clap` CLI, config, Credential Manager, rendering, commands | Denied |

Planned crates for later milestones: `raven-git` (checkpoints), `raven-mcp`,
`raven-policy` (if policy outgrows core), `raven-tui`, `raven-plugin`.

## Core loop

1. Load configuration and resolve the provider, model, endpoint, and key.
2. Create a session and append `session_started`.
3. Build a system prompt from fixed rules plus an optional `AGENTS.md` /
   `CLAUDE.md`, labeled as untrusted repository content.
4. Stream a chat completion. Text deltas go to the observer; tool-call deltas
   are assembled per index.
5. If the model returned tool calls, validate JSON arguments, check policy,
   execute read-only tools, append results, and loop.
6. Stop on a final assistant message, a policy/budget/turn error, or
   cancellation.

Every step is appended to the event log. Cancellation is checked before each
turn, between stream items, and before each tool call. Provider streams select
on a `CancellationToken` so Ctrl+C interrupts an in-flight read.

### Traits

- `Provider` — streaming chat completion; provider-specific events normalized.
- `Tool` — spec, kind, async call with a `ToolContext`.
- `EventSink` — append-only audit events.
- `Observer` — terminal/JSON rendering of streamed text and tool activity.
- Planned: `Sandbox`, `CheckpointStore`.

## Provider layer

One adapter covers every OpenAI-compatible endpoint. Capability differences
are configuration, not code paths:

- `base_url` override and `/chat/completions` append.
- `stream_usage` toggle for gateways that reject `stream_options`.
- Tool-call deltas may arrive fragmented, with or without IDs, and with
  arguments as a JSON string or object; the adapter normalizes them.
- Retries (3 attempts) for connect errors, timeouts, HTTP 408, 429, and 5xx,
  honoring `Retry-After`. Retries happen before the stream starts; a mid-stream
  failure is surfaced rather than replayed to avoid duplicate tool calls.
- 180-second idle timeout on an open stream.

Usage is normalized to input/output/cached tokens. Pricing is user-supplied in
USD per million tokens; cost is computed only when pricing exists.

## Tools and policy

M0 registers only `read_file`, `list_dir`, and `search_files`. M1 adds
`edit_file` and `create_file`; execute and network tools are still refused.
Read confinement is unchanged; write paths are described in "Edits (M1)". The
policy
rejects write/execute/network tool kinds even if such a tool were registered by
mistake.

`PathPolicy`:

- Canonicalizes the workspace root and every requested path with `dunce`.
- Rejects relative traversal and paths outside the root.
- Rejects device paths (`\\.\`, `\\?\GLOBALROOT`, named pipes).
- Denies `.git` components and secret-bearing filenames (`.env*`, private keys,
  `.npmrc`, `.pypirc`, credential stores).

Tool output is sanitized: ANSI CSI/OSC and other control characters are
removed, binary files are rejected, reads are capped at 4 MiB, and output is
capped by `output_cap` (64 KiB default). Tool results are redacted again before
they enter the conversation or the log.

This policy is a usability and accident-prevention layer, not an OS boundary.
The boundary arrives with the sandbox.

## Event store

M0 and M1 use `events.jsonl` plus `meta.json` per session as the source of truth.
Each event is
hashed with SHA-256 over a canonical serialization that includes the previous
hash. `seq` starts at 1; `prev_hash` starts at 64 zeroes. Reads verify sequence,
links, and hashes.

Why no SQLite in M0: the read-only loop does not need indexed queries, and the
log must stay replayable without a database. M1 will add a SQLite projection for
search and resume, while keeping JSONL authoritative. Rebuilds will be possible
from the log alone.

Redaction happens before serialization, so secrets should not reach the log even
if a tool returns them. The log is tamper-evident (chain) but not tamper-proof
(an attacker with full write access can rebuild the chain).

`meta.json` holds derived session metadata (provider, model, status, turn and
token totals). It is written to `meta.json.tmp` and renamed over the
destination. On Windows, `rename` does not reliably replace an existing file, so
the fallback removes the destination and retries. That leaves a short window
where `meta.json` is absent and a crash during the fallback can leave the
previous metadata behind. The event log is authoritative; `meta.json` is a
cache that can be rebuilt from it. Session IDs are validated before use to block
path traversal.

## Sandbox plan

M0 only probes support and never executes commands. The backends, in order:

1. **MXC** — preferred where the OS build and policy support it. No admin setup.
2. **AppContainer + Job Object + restricted token** — fallback. Job
   `KILL_ON_JOB_CLOSE`, sanitized environment, explicit filesystem grants.
3. **Best-effort** — policy and audit only, clearly labeled "not a boundary".
4. **Off** — explicit opt-in only.
5. **VM** — optional later tier for untrusted repositories.

Each backend must report its actual guarantees in `doctor`, pass an escape
suite, and carry a compatibility profile for npm/cargo/Python/git. Codex's
native Windows backends mean containment is table stakes, not a wedge; see
`docs/codex-windows-status.md`.

Unsafe policy: `#![deny(unsafe_code)]` in every crate. When containment lands,
`raven-sandbox` is the only crate allowed to relax it, with a documented
justification per block. No other crate may call Windows APIs directly.

## Edits (M1)

Exact `old`/`new` search-replace only, never line numbers:

- Read the file, verify the expected post-state, and require the old text to be
  unique.
- Preserve EOL style, BOM, and trailing-newline convention.
- Write to a temporary file in the same directory, flush and `fsync`, then
  replace the original with `ReplaceFileW` (or `MoveFileExW` with
  `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`). If the replace cannot
  be made atomic on the target filesystem, fail rather than fall back to
  delete-then-rename.
- Re-read and hash-verify the result.
- Record before/after hashes and the diff in the audit log.

Path policy is checked again at write time (TOCTOU-aware), and symlink/junction
targets are resolved. A failed verification leaves the original untouched.

## Approval model (M2)

M1 approvals are all-or-nothing and that is sufficient for files: an edit either
gets shown and approved or it does not. Commands need a spectrum — `cargo test`
and `rm -rf` are both "execute" and must not get the same answer. M2 introduces
the ruleset.

### Model

A rule is `(subject, pattern, action)`:

| Field | M2 value |
| --- | --- |
| subject | `command`, `write-path`, `read-path` |
| pattern | Glob-ish match against the command line, or the resolved path |
| action | `Allow`, `Ask`, `Deny` |

Every request is evaluated against the layers in this order, first match wins:

1. **Hard rules** — compiled in, never writable and never overridable. Protect
   the invariant rather than express a preference: protected paths (`.git`,
   `.env*`, key files) are always `Deny`; credential selection
   (`default_provider`, `api_key_env`) can only come from the user config; no
   rule can grant network access. A rule set that would auto-approve everything
   is rejected at parse time, so a typo cannot become a blanket allow.
2. **User config rules** — the user's own `raven.toml` or an `--allow` flag.
   Highest-trust source because the user owns the file.
3. **Session rules** — `a`/`all` at an approval prompt, kept in memory for the
   run and discarded on exit. Raven already has this shape: the approver holds
   an `AtomicBool` today and it should become the per-pattern version.
4. **Project rules** — only if the workspace is trusted (see the trust gate).
   Least trusted: a random repository must not be able to widen the rule set,
   so a project file may not add `Allow` rules that the user has not also
   authorized, and any project rule matching a hard rule is dropped with a
   warning.
5. **Ask** — the fallback. `Ask` is a request for a human decision, not a
   weakened allow.

### Why falls back to Ask, never Allow

The agent must never fail open. When no layer matches, the action is `Deny`
unless a human says otherwise, so an unmatched request prompts and a denied
prompt means not executed. This is the same invariant as M1's `DenyAll`
default, extended per-subject instead of per-tool-kind.

### Headless

`Ask` in a non-interactive run is `Deny` with a message saying re-run
interactively or pass an explicit rule. M6 must keep this: a CI run that
auto-approves because stdin was connected to a pipe is the same failure as
blanket approval. Detecting this with `stdin().is_terminal()` is already what
`ConsoleApprover` does for edits.

### Out of scope for M2

**`drain covered`** — auto-resolving queued requests once a widening rule is
approved — requires concurrent tool dispatch. Raven's agent loop runs tool
calls sequentially, so no request is ever pending. If M4-M8 introduce
concurrency this becomes required, and the design note is: when a rule
widens, re-evaluate queued requests and log both the original ask and the
reason it was auto-resolved.

### Attack surface to hold in check

A ruleset is more configurable than a mode, which means more ways to
mis-express an allow. Hard rules must therefore be few, trivially auditable,
and covered by tests that assert a crafted rule set still cannot exceed them:
`Allow` for every command, a path glob that swallows protected names via
trailing dots or ADS suffixes, a project-supplied rule, and a rule that
narrows the hard set.

## Git checkpoints (M4)

Use `git commit-tree` with a temporary index and temporary refs; never touch the
user's HEAD, index, or working tree during checkpoint creation. Restore is an
explicit operation. Non-git workspaces get snapshot-based checkpoints.

## MCP (M5)

Allowlisted servers only, hash-pinned executable/config, per-server environment
allowlist, secret scrubbing, sandboxed child process, and the M1 workspace trust
gate extended to project-level MCP configuration. Tool descriptions and results
are untrusted input, never capability grants.

## Tool output and terminal safety

All tool output is treated as untrusted: control sequences stripped, size
capped, binary content rejected. The renderer never interprets tool text as
commands. JSON output uses one event per line and keeps diagnostics on stderr.

## Dependencies

Current: `tokio`, `reqwest` (native TLS), `eventsource-stream`, `clap`,
`serde`/`serde_json`, `toml`, `keyring` (Windows native), `ignore`, `globset`,
`regex`, `sha2`, `uuid`, `chrono`, `dunce`, `rpassword`.

Planned: `rusqlite` (bundled) for the M1 projection, `gix` read-only + git CLI
for checkpoints, `portable-pty` for ConPTY, `ratatui` for a TUI, `windows` /
`windows-sys` for containment in `raven-sandbox`.

## No daemon in v1

The CLI is a normal process with no listening socket. If IDE integration later
requires one, use a named pipe with an explicit ACL and per-user authentication,
not loopback TCP.
