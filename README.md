# Raven CLI

Windows-first, provider-neutral coding agent. One Rust binary, no runtime, no
daemon, no WSL requirement.

> **Status: M0 implemented — read-only.**
> Raven can stream from an OpenAI-compatible provider and inspect a workspace
> with three read-only tools. It cannot edit files, execute commands, run tests,
> use MCP, or create git checkpoints yet. There is no OS sandbox because the
> agent has no command-execution tool; the only child process is `git --version`
> in `doctor`. See [Security posture](#security-posture-m0).

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

## What works in M0

- Streaming chat completions over the OpenAI-compatible `/chat/completions`
  SSE protocol, with tool-call delta assembly.
- Tool loop with `read_file`, `list_dir`, and `search_files` (regex).
- Workspace path policy: canonicalized paths, no traversal, no device paths,
  denylist for `.git`, `.env*`, keys, and credential files.
- Secret redaction for common API keys, JWTs, bearer tokens, private keys, and
  URL credentials. Tool results and repository instructions are redacted before
  provider egress; event data is redacted before log writes. User prompts are
  sent as typed.
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

Not implemented yet: edits, command execution, OS sandboxing, git checkpoints,
MCP, skills, plugins, TUI, provider cassette record/replay.

## Quickstart

### 1. Prerequisites

- Rust MSVC toolchain (1.80+; tested with 1.91.1)
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
| `raven run [OPTIONS] <PROMPT>...` | Run one read-only agent turn. |
| `raven init [--force]` | Write a sample `raven.toml`. |
| `raven auth set <provider> [--stdin]` | Store a key in Credential Manager. |
| `raven auth status` | Show which providers have credentials. |
| `raven auth delete <provider>` | Remove a stored key. |
| `raven doctor` | Check config, credentials, workspace, git, and sandbox probes. |
| `raven sessions [--json] [--limit N]` | List recent sessions. |
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

Project-local `raven.toml` files are **ignored** until a workspace trust gate
exists (planned for M1). A cloned repository must not be able to choose your
endpoint or key source.

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

## Security posture (M0)

M0 is a read-only agent. That is a capability limit, not a claim that the
process is isolated.

**Enforced now**

- Only read-only tools are registered. The policy also rejects write, execute,
  and network tool kinds.
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
  ├── raven-tools     read-only tools (read_file, list_dir, search_files)
  ├── raven-store     JSONL hash-chained event log + session metadata
  └── raven-sandbox   probe/planning stub; all future Windows FFI lives here
```

Dependency direction is one-way: `raven-core` has no dependency on providers,
tools, storage, or Windows APIs. `raven-sandbox` denies `unsafe_code` in M0 and
will be the only crate allowed to relax that when containment lands.

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

The e2e script starts `scripts/mock_provider.py` on `127.0.0.1:8765`, runs a
two-turn session that calls `read_file`, then verifies the session list and
event log. It uses temporary config and data directories and prefers
`target\debug\raven.exe`, falling back to `target\release\raven.exe`.

## Roadmap

| Milestone | Scope | Status |
| --- | --- | --- |
| M0 | Provider streaming + read-only loop, cancellation, accounting, event log | Implemented |
| M1 | Safe edits, workspace-write policy, audit hardening, secret canaries | Next |
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
