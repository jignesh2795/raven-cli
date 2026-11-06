# Raven CLI

Windows-first, provider-neutral coding agent. One Rust binary, no runtime, no
daemon, no WSL requirement.

> **Status: M1 implemented — streaming, reading, and exact file edits.**
> Raven streams from an OpenAI-compatible provider, inspects a workspace with
> three read-only tools, and applies exact search/replace edits. Every edit is
> atomic, hash-verified, shown to you as a diff, and requires approval before it
> is written (`--yes` skips the prompt; non-interactive runs fail closed). It
> cannot execute commands, run tests, use MCP, or create git checkpoints yet.
> There is still no OS sandbox — see [Security posture](#security-posture-m1).

## Why Raven

Native Windows sandboxing is no longer an open gap: OpenAI Codex now ships
`mxc`, `elevated`, and `unelevated` backends. Raven does not claim to
out-sandbox Codex. The bet is different:

- **Provider-neutral.** OpenAI, NVIDIA NIM, Ollama, and any OpenAI-compatible
  endpoint. Bring your own key; local models need no key. No vendor account is
  required.
- **Local-first.** Works offline against a local model server. No telemetry, no
  control plane, no daemon.
- **Auditable.** A hash-chained event log per session, a documented threat
  model, and open tests you can run yourself.
- **Enterprise-friendly.** User-owned configuration, fail-closed defaults,
  strict path policy, and honest labels for every guarantee.
- **Windows-native.** Credential Manager for keys, native paths, a single
  `raven.exe`, and no WSL dependency.

The sandbox research that shaped this positioning is in
[`docs/codex-windows-status.md`](docs/codex-windows-status.md).

## What works in M1

- Streaming chat completions over the OpenAI-compatible `/chat/completions`
  SSE protocol, with tool-call delta assembly.
- Tool loop with `read_file`, `list_dir`, and `search_files` (regex).
- Exact file edits: `edit_file` (uniqueness-checked search/replace) and
  `create_file` (never overwrites). UTF-8 BOM and line-ending style are
  preserved, writes are atomic (`ReplaceFileW`/`MoveFileExW`), and the result is
  re-read and hash-verified. Before/after SHA-256 hashes land in the audit log.
- Approval gate: every change is previewed as a unified diff and needs
  `y`/`n`/`a`. `--yes` approves unattended; a non-interactive run without
  `--yes` applies nothing.
- `--read-only` refuses all writes for a session; `policy = "read-only"` does it
  in `raven.toml`.
- Workspace path policy for writes: canonicalized parents, protected-name
  normalization (including `.env::$DATA` and trailing-dot bypasses), reserved
  Windows names, symlinks and junctions resolved before the check.
- Secret redaction for common API keys, JWTs, bearer tokens, private keys, and
  URL credentials. Tool results and repository instructions are redacted before
  provider egress; event data is redacted before log writes. Approval diffs are
  redacted too. User prompts are sent as typed.
- Terminal escape stripping and output caps for untrusted file content, tool
  output, and streamed model text.
- Ctrl+C cancellation (second Ctrl+C exits immediately).
- Token usage and optional per-model cost accounting, with a cap checked after
  each model response.
- Append-only, SHA-256 hash-chained session log plus `meta.json`; `show`
  verifies the chain.
- Human-readable stream output and versioned JSONL for automation.
- `doctor`, `init`, `auth`, `sessions`, and `show` commands.
- End-to-end smoke test against a local mock provider.
- Secret canary suite proving planted credentials never reach a provider
  payload, the audit log, or an approval diff.

Not implemented yet: command execution, OS sandboxing, git checkpoints,
MCP, skills, plugins, TUI. Cassette record/replay is done (below).

## Quickstart

### 1. Prerequisites

- Rust MSVC toolchain (1.80+; tested with 1.99.1)
- Visual Studio Build Tools with MSVC x64 and the Windows 10/11 SDK
- Python 3 only for the end-to-end test

If `cargo` or `link.exe` is not on `PATH`, configure the shell:

```powershell
. .\scripts\dev-env.ps1
```

If the Windows SDK is missing entirely, use the no-admin NuGet fallback. It
downloads the SDK link libraries once into `%LOCALAPPDATA%\raven-sdk`; after
that, plain `.\scripts\dev-env.ps1` finds them automatically:

```powershell
. .\scripts\dev-env.ps1 -DownloadSdk
```

Full setup and troubleshooting: [`docs/build.md`](docs/build.md).

### 2. Build

```powershell
cargo build --release
.\target\release\raven.exe --version
```

The examples below use `raven` for brevity. Add `target\release` to `PATH` or
call `.\target\release\raven.exe` directly.

### 3. Configure and authenticate

```powershell
raven init
raven auth set openai
raven doctor
```

`raven auth set` stores the key in Windows Credential Manager under the
service `raven-cli`. Environment variables also work:
`RAVEN_API_KEY`, `OPENAI_API_KEY`, or the provider's configured `api_key_env`.

### 4. Ask a question

```powershell
raven run "Summarize this repository and list the main crates"
```

Options must come before the prompt because the prompt is a trailing argument:

```powershell
raven run --provider ollama --model qwen2.5-coder:7b --workspace C:\src\app "Where is auth implemented?"
```

Local Ollama requires the server to be running and an OpenAI-compatible
endpoint at `http://localhost:11434/v1` (the default preset).

### 5. Inspect the session

```powershell
raven sessions
raven show <session-id>
```

## Commands

| Command | Purpose |
| --- | --- |
| `raven run [OPTIONS] <PROMPT>...` | Run one agent turn: reads, and exact edits with approval. |
| `raven init [--force]` | Write a sample `raven.toml`. |
| `raven auth set <provider> [--stdin]` | Store a key in Credential Manager. |
| `raven auth status` | Show which providers have credentials. |
| `raven auth delete <provider>` | Remove a stored key. |
| `raven doctor` | Check config, credentials, workspace, git, and sandbox probes. |
| `raven sessions [--json] [--limit N]` | List recent sessions. |
| `raven trust [--grant \| --revoke] [--workspace PATH]` | Review, trust, or revoke a project-local `raven.toml`. |
| `raven show <session-id> [--json]` | Print a session with chain verification. |

### `run` options

| Option | Meaning |
| --- | --- |
| `--workspace <PATH>` | Root for tool access. Defaults to the current directory. |
| `--provider <NAME>` | `openai`, `nim`, `ollama`, or a configured provider. |
| `--model <MODEL>` | Override the provider's model. |
| `--base-url <URL>` | Override the OpenAI-compatible base URL. |
| `--api-key-env <VAR>` | Read the key from this environment variable. |
| `--max-turns <N>` | Agent loop limit. Default 12. |
| `--read-only` | Refuse every file write this run, ignoring the configured policy. |
| `--yes`, `-y` | Approve every edit without prompting. Only for trusted workspaces; conflicts with `--read-only`. |
| `--record <PATH>` | Capture this run's provider responses to a cassette for later replay. |
| `--replay <PATH>` | Replay a recorded cassette instead of calling the provider. |
| `--max-cost <USD>` | Stop after a response pushes the total over this cap. Requires a matching pricing entry. |
| `--temperature <F>` / `--max-tokens <N>` | Sampling controls. |
| `--json` | Emit versioned JSONL on stdout. |
| `--no-color` | Disable ANSI color in human output. |

## Configuration

Default path: `%APPDATA%\raven\raven.toml`. Override with `--config` or
`RAVEN_CONFIG`. Session data defaults to `%LOCALAPPDATA%\raven`; override with
`RAVEN_DATA_DIR`.

```toml
default_provider = "openai"
max_turns = 12
output_cap = 65536
# "workspace-write" allows exact edits (each one needs approval unless --yes).
# "read-only" refuses all writes.
policy = "workspace-write"
redact = true
# max_cost_usd = 1.50
# temperature = 0.2

[providers.openai]
base_url = "https://api.openai.com/v1"
model = "gpt-4.1"
api_key_env = "OPENAI_API_KEY"

[providers.nim]
base_url = "https://integrate.api.nvidia.com/v1"
model = "meta/llama-3.3-70b-instruct"
api_key_env = "NVIDIA_API_KEY"

[providers.ollama]
base_url = "http://localhost:11434/v1"
model = "qwen2.5-coder:7b"
stream_usage = false

[pricing."gpt-4.1"]
input_per_mtok = 2.00
output_per_mtok = 8.00
cached_input_per_mtok = 0.50
```

Key resolution order: `RAVEN_API_KEY`, then the provider's `api_key_env` (or
`--api-key-env`), then Windows Credential Manager. A missing key is allowed for
localhost endpoints.

Pricing is in USD per million tokens and is deliberately user-supplied.
Prices change; hard-coded vendor prices would go stale silently. `--max-cost`
refuses to run if the model has no pricing entry. The cap is checked after each
model response, so a single response can push usage past the limit before the
session stops.

`redact = false` disables pattern redaction for tool results, repository
instructions, logs, and display. It exists for debugging only; do not use it
with untrusted repositories or real credentials.

## Project-local configuration

A repository can ship its own `raven.toml`. It is **ignored** until you trust
that workspace:

```powershell
raven trust                 # show what it would change, and the current state
raven trust --grant         # review it and trust it (interactive)
raven trust --revoke        # stop honoring it
```

Trust is stored per workspace and pinned to the SHA-256 of the exact file you
reviewed, so editing `raven.toml` invalidates trust and the file is ignored
again until you re-trust it. Granting requires a terminal; a non-interactive run
fails closed. A trusted project file can supply safe defaults such as
`max_turns` and `policy`, add a custom provider, or fill unset non-credential
fields on a provider you explicitly configured. It cannot choose
`default_provider`, replace a built-in provider preset, set `api_key_env`,
override pricing, or change redaction. Credential sources remain user-owned:
`RAVEN_API_KEY`, the user-configured provider environment variable, or the
provider's Windows Credential Manager entry. A project-supplied
`base_url` is a credential destination: when you select that provider, Raven
sends its resolved key to that endpoint. Review provider URLs before granting
trust. `raven doctor` reports whether a project file is being used.

## Sessions and audit

Each run writes:

```text
%LOCALAPPDATA%\raven\sessions\<session-id>\
  meta.json      # provider, model, status, turns, usage, cost
  events.jsonl   # append-only, SHA-256 hash-chained events
```

Every event carries `v`, `seq`, `id`, `session_id`, `ts`, `kind`, `summary`,
`data`, `prev_hash`, and `hash`. `raven show` verifies sequence numbers,
previous-hash links, and each event hash before printing. This is
**tamper-evident**, not tamper-proof: someone who can rewrite the whole log can
forge a new chain, but accidental corruption and naive edits are detected.

The log records session start, user prompt, provider requests, assistant text
and tool calls, tool results, usage, notices, errors, and the final status.

## JSON output

With `--json`, stdout is versioned JSONL (`"v": 1`) and diagnostics go to
stderr. Event types:

| Type | Fields |
| --- | --- |
| `session_started` | `session_id`, `provider`, `model`, `workspace` |
| `text_delta` | `text` |
| `tool_call` | `name`, `arguments` |
| `tool_result` | `name`, `ok`, `summary` |
| `notice` | `message` |
| `done` | `session_id`, `turns`, `finish_reason`, token counts, `cost_usd`, `cost_known` |
| `error` | `session_id`, `message`, `exit_code` |

Example (one JSON object per line):

```powershell
raven run --json "List the crates"
```

Exit codes:

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | Configuration, provider, tool, or I/O error |
| 3 | Policy error that aborts the run (tool-level denials are normally returned to the model) |
| 4 | Cost budget exceeded |
| 5 | Maximum turns reached |
| 130 | Cancelled with Ctrl+C |

## Security posture (M1)

M1 adds file writes under explicit approval. Every write is still a
human-authorized, atomic, hash-verified operation, but it is a capability
increase over M0 and is described honestly below.

**Enforced now**

- Read tools always; write tools only under `policy = "workspace-write"`.
  `read-only` registers no write tool at all, and the policy still denies
  writes at execution time as a second layer.
- Tool paths are canonicalized and confined to the workspace; traversal,
  device paths, and protected paths (`.git`, `.env*`, key files, credential
  files) are denied.
- Common secrets are redacted from tool results and repository instructions
  before provider egress, and from event data before log writes. User prompts
  are sent as typed. Rendering is redacted and control-stripped.
- File content, tool output, and streamed model text are ANSI/OSC-stripped;
  output is capped (4 MiB read, 64 KiB tool output by default).
- No project config is loaded; no daemon; no listening socket.
- The event log is hash-chained and verified on read.
- Turn and cost limits fail closed.

**Not claimed**

- No OS-level sandbox or process containment. The agent loop starts no child
  processes; `doctor` only runs `git --version`.
- Path policy and redaction are in-process mitigations, not a boundary against
  a compromised binary, malicious local process, or kernel attacker.
- Redaction is pattern-based and best-effort. It will miss unknown formats.
- A compromised dependency could ignore policy. Reproducible builds and
  dependency review matter.

The sandbox plan is MXC first, AppContainer + Job Object + restricted token as
a fallback, each with an escape suite and compatibility profile. See
[`SECURITY.md`](SECURITY.md) and [`THREAT_MODEL.md`](THREAT_MODEL.md).

## Architecture

```text
raven (CLI)
  ├── raven-core      agent loop, events, policy, redaction, cost, traits
  ├── raven-provider  OpenAI-compatible SSE adapter (OpenAI/NIM/Ollama/custom)
  ├── raven-tools     read tools, exact edits (edit_file, create_file)
  ├── raven-store     JSONL hash-chained event log + session metadata
  └── raven-sandbox   Windows FFI home: atomic file replace today, containment later
```

Dependency direction is one-way: `raven-core` has no dependency on providers,
tools, storage, or Windows APIs. `raven-sandbox` is the only crate allowed to
use `unsafe`: it hosts the atomic-replace FFI today and containment later.
Every other crate denies `unsafe_code`.

More detail: [`DESIGN.md`](DESIGN.md).

## Development

```powershell
. .\scripts\dev-env.ps1
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build
powershell -ExecutionPolicy Bypass -File .\scripts\e2e.ps1
```

The e2e script starts `scripts/mock_provider.py` on `127.0.0.1:8765`, then runs
a session that reads, edits, and creates files, and asserts the edit landed, the
created file exists, and the audit log contains the `edit_applied` event. It
uses temporary config, data, and workspace directories and prefers
`target\debug\raven.exe`, falling back to `target\release\raven.exe`.

The secret canary suite is a normal test target:

```powershell
cargo test -p raven-cli --test canary
```

## Provider cassettes

Record a real session once and replay it deterministically, with no network and
no API key:

```powershell
raven run --record session.jsonl "Fix the failing test"
raven run --replay session.jsonl "Fix the failing test"
```

Replay runs through the *same* SSE parser as the live path, so a cassette
exercises the real framing, `[DONE]` handling, tool-call delta assembly, and
usage accounting rather than skipping them. Matching is by the shape of the
conversation — how many turns happened, which tools were called in what order —
so a differently worded prompt still replays, but a conversation that has
diverged is refused rather than answered against the wrong entry.

A cassette holds whatever the provider sent, which can include your prompt text.
Treat it like a session transcript: do not commit one that contains credentials
or private code.

## Roadmap

| Milestone | Scope | Status |
| --- | --- | --- |
| M0 | Provider streaming + read-only loop, cancellation, accounting, event log | Implemented |
| M1 | Safe edits, workspace-write policy, audit hardening, secret canaries, trust gate | Implemented (SQLite, cassettes, entropy remaining) |
| M2 | Command runner, best-effort containment, output limits | Planned |
| M3 | MXC + AppContainer sandbox, compatibility profiles, escape suite | Planned |
| M4 | Git checkpoints and undo | Planned |
| M5 | MCP with trust gate and hash pinning | Planned |
| M6 | Headless/CI hardening | Planned |
| M7 | Skills | Planned |
| M8 | Plugins | Planned |

See [`MILESTONES.md`](MILESTONES.md) for exit criteria.

## License

MIT OR Apache-2.0.
