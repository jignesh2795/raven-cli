# Final validation evidence: Raven security audit batch 1

## Revision identity

| Item | Value |
| --- | --- |
| Repository | `https://github.com/jignesh2795/raven-cli` |
| Branch | `fix/audit-h1-h3-create-only-v1` |
| Validated implementation SHA | `90c9a890d3cab1e8b7f327ac18b06a0844c7ef53` |
| Remote branch HEAD at validation | `90c9a890d3cab1e8b7f327ac18b06a0844c7ef53` |
| Local branch HEAD at validation | `90c9a890d3cab1e8b7f327ac18b06a0844c7ef53` |
| Base | `5852d53dbd37a8b6171a430860c350356abc0c9a` |
| `main` (local and remote) | `5852d53dbd37a8b6171a430860c350356abc0c9a` |
| Base ancestry | Preserved (`git merge-base --is-ancestor`, exit `0`) |
| Working tree before validation | Clean |

The local branch was fast-forwarded from the previous evidence commit
`b2a41f255e74f549aabcc4f2a4eda052c0ecc373` to the exact target. No reset,
rebase, force-push, merge, or PR was performed. The implementation commit
`90c9a89` changed only `crates/raven-cli/src/config.rs` (10 insertions,
9 deletions) to apply the exact rustfmt output.

**The evidence commit is separate from the validated implementation SHA.** It
is a child commit containing only this document and does not alter the
implementation at `90c9a890d3cab1e8b7f327ac18b06a0844c7ef53`.

## Environment

| Item | Value |
| --- | --- |
| OS | Microsoft Windows 10 Pro, version `10.0.19045`, build `19045`, 64-bit |
| Rust | `rustc 1.99.1 (58cf8f9c3 2026-10-13)` |
| Cargo | `cargo 1.99.1 (5f94df478 2026-08-27)` |
| rustfmt | `rustfmt 1.10.0-stable (58cf8f9c35 2026-10-13)` |
| Active toolchain | `stable-x86_64-pc-windows-msvc` (default) |
| Host / target triple | `x86_64-pc-windows-msvc` |
| MSVC tools | `14.29.30133` (`Hostx64\x64`) |
| Windows SDK | `10.0.28000.2957` (NuGet fallback under `%LOCALAPPDATA%\raven-sdk`) |

All Cargo commands were run after dot-sourcing `scripts/dev-env.ps1`.

## Formatting gate — PASS

Command:

```powershell
cargo fmt --all -- --check
```

Exit code: **0**

No formatter diff was produced. The four previously reported corrections in
`crates/raven-cli/src/config.rs` are now accepted by rustfmt:

- `resolve_provider` chain joined onto one line.
- `api_key_env` assertion wrapped across lines.
- `pricing.get` chain joined onto one line.
- `effective_redact` assertion wrapped across lines.

No `cargo fmt` write operation was performed during validation.

## Focused regression tests — all pass

| Command | Result | Exit |
| --- | --- | --- |
| `cargo test -p raven-cli --test cli_regressions` | 6 passed, 0 failed | 0 |
| `cargo test -p raven-cli trusted_project_cannot_override_user_provider_credentials_pricing_or_redaction` | 1 passed, 0 failed | 0 |
| `cargo test -p raven-sandbox create_only` | 2 passed, 0 failed | 0 |
| `cargo test -p raven-tools` | 14 passed, 0 failed | 0 |

The CLI regression run exercised the loopback mock provider and isolated
temporary state without real credentials or external services.

## Full validation matrix

| Command | Result | Exit |
| --- | --- | --- |
| `cargo fmt --all -- --check` | Clean | 0 |
| `cargo test --workspace` | 89 passed, 0 failed | 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | Clean | 0 |
| `cargo build --workspace --release` | Clean | 0 |
| `powershell -ExecutionPolicy Bypass -File scripts/e2e.ps1` | `E2E OK` (session `20261106-153754-fab374`) | 0 |
| `git diff --check` | Clean | 0 |

Workspace test breakdown: raven-cli unit `9`, canary `5`, CLI regressions `6`,
raven-core `36`, raven-provider `13`, raven-sandbox `4`, raven-store `2`,
raven-tools `14` (89 total). Doc-tests: 0.

The E2E run exercised `read_file`, `edit_file`, `create_file`, approval, audit
logging, and session reporting against the local mock provider (4 turns).

## Security and behavior invariants

- **H1:** Project configuration cannot override user-owned provider credentials,
  built-in provider presets, pricing, the default provider, or redaction
  behavior. The focused H1 unit test passed. The CLI `--max-cost` regression
  also passed, confirming project pricing cannot bypass the cost check.
- **H2:** Cassette replay works without an API key and does not contact the
  configured provider. The live-provider path still fails closed without
  credentials. Both CLI regressions passed.
- **H3:** JSONL stdout remains protocol-clean, with every line valid JSON carrying
  `v = 1`; recorder success and failure diagnostics appear on stderr. Both
  JSONL regressions and the human-readable regression passed.
- **M1:** Create-only writes preserve an existing or unexpectedly appearing
  target, temporary files are cleaned up, and normal edit replacement still
  works. Native sandbox tests, tool-level atomic-write tests, and the
  approval-race test passed.

## Windows-specific verification

Validation ran natively on Windows MSVC. `crates/raven-sandbox/src/atomic.rs`
calls `MoveFileExW` with `MOVEFILE_WRITE_THROUGH` only; the `REPLACE_EXISTING`
flag is not passed, so an unexpected target causes an error instead of being
replaced. The native create-only tests and the approval-race test passed.

The Unix `hard_link` create path was source-reviewed but not executed on this
Windows host. No Unix runtime validation is claimed.

## Unavailable / skipped checks

- Non-Windows execution was not available on this host.
- No GitHub Actions, paid CI, paid APIs, or external services were used.

## Verdict

**PASS** — every required gate passed at the exact implementation SHA
`90c9a890d3cab1e8b7f327ac18b06a0844c7ef53`: formatting, focused regressions,
full workspace tests, Clippy with warnings denied, release build, E2E, and
whitespace checks. No production or test source was modified during this
validation; only this evidence document is published.
