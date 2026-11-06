# Validation evidence: initial Raven security audit fixes

## Revision identity

| Item | Value |
| --- | --- |
| Repository | `https://github.com/jignesh2795/raven-cli` |
| Branch | `fix/audit-h1-h3-create-only-v1` |
| Validated starting HEAD | `7eaa37213cfc9cdc2b3a28115d9d5d46d65dee37` |
| Base | `5852d53dbd37a8b6171a430860c350356abc0c9a` |
| Remote HEAD at validation start | `7eaa37213cfc9cdc2b3a28115d9d5d46d65dee37` |
| Base is an ancestor of HEAD | Yes (`git merge-base --is-ancestor`, exit `0`) |
| Working tree before validation | Clean |

The branch was checked out from `origin/fix/audit-h1-h3-create-only-v1` without
reset, rebase, or force-push. No production implementation was modified during
validation. The test-only additions and this report are published as a child
commit of the validated SHA above.

## Environment

| Item | Value |
| --- | --- |
| OS | Microsoft Windows 10 Pro, version `10.0.19045`, build `19045`, 64-bit |
| Kernel string | `Microsoft Windows NT 10.0.19045.0` |
| Rust | `rustc 1.99.1 (58cf8f9c3 2026-10-13)` |
| Cargo | `cargo 1.99.1 (5f94df478 2026-08-27)` |
| Active toolchain | `stable-x86_64-pc-windows-msvc` (default) |
| Host / target triple | `x86_64-pc-windows-msvc` |
| MSVC tools | `14.29.30133` (`Hostx64\x64`) |
| Windows SDK | `10.0.28000.2957` (NuGet fallback under `%LOCALAPPDATA%\raven-sdk`) |

All cargo commands were run after dot-sourcing `scripts/dev-env.ps1`, which puts
the native MSVC linker and SDK libraries on `PATH`/`LIB`.

## Focused regressions

| Command | Result | Exit |
| --- | --- | --- |
| `cargo test -p raven-cli --test cli_regressions` | 6 passed, 0 failed | 0 |
| `cargo test -p raven-cli trusted_project_cannot_override_user_provider_credentials_pricing_or_redaction` | 1 passed, 0 failed | 0 |
| `cargo test -p raven-sandbox create_only` | 2 passed, 0 failed | 0 |
| `cargo test -p raven-tools` | 14 passed, 0 failed | 0 |

## Full validation matrix

| Command | Result | Exit |
| --- | --- | --- |
| `cargo fmt --all -- --check` | **FAIL** — formatting diffs in production files | **1** |
| `cargo test --workspace` | 89 passed, 0 failed | 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | Clean | 0 |
| `cargo build --workspace --release` | Clean | 0 |
| `powershell -ExecutionPolicy Bypass -File scripts/e2e.ps1` | `E2E OK` (4 turns; read/edit/create/audit) | 0 |
| `git diff --check` | Clean | 0 |

Workspace test breakdown: raven-cli unit `9`, canary `5`, CLI regressions `6`,
raven-core `36`, raven-provider `13`, raven-sandbox `4`, raven-store `2`,
raven-tools `14` (89 total). Doc-tests: 0.

### Formatting failure detail

`cargo fmt --all -- --check` fails in two production files introduced/modified
by the branch's fix commits:

- `crates/raven-cli/src/config.rs:588`
- `crates/raven-cli/src/config.rs:595`
- `crates/raven-cli/src/trust.rs:273`

These lines are additions in the validated branch (they are absent from base
`5852d53`), so this is a branch-level formatting failure rather than a
pre-existing baseline failure. The validation changes themselves are formatted:
the new `crates/raven-cli/tests/cli_regressions.rs` was formatted with
`rustfmt --edition 2021` and is not reported by the check.

The failures are in production code, which this task explicitly forbids
changing. They were therefore recorded rather than fixed.

## Windows-specific verification

- Validation ran natively on `x86_64-pc-windows-msvc`; no Linux-only results
  are claimed.
- `crates/raven-sandbox/src/atomic.rs` implements create-only persistence with
  `MoveFileExW(source, destination, MOVEFILE_WRITE_THROUGH)`. The
  `REPLACE_EXISTING` flag is not passed, so an unexpected target causes an
  error instead of replacement.
- The native Windows tests `create_only_creates_a_missing_target` and
  `create_only_refuses_to_replace_an_unexpected_target` passed.
- The tool-level approval-race test `a_target_appearing_during_approval_is_preserved`
  passed and verified the raced file remained byte-for-byte intact with no
  `.raven-tmp-*` leftovers.
- The Unix implementation was source-reviewed but not executed on this Windows
  host; cross-platform execution was not available and is recorded as not run.

## Added test coverage

### `crates/raven-cli/tests/cli_regressions.rs` (new, 6 tests)

Uses the built `raven` binary, a loopback `TcpListener` mock provider, and
isolated temporary directories. No real API keys or external services are used.

- `replay_works_without_an_api_key_and_does_not_contact_the_provider` (H2):
  records against the loopback provider, then replays with all provider keys
  removed and an unreachable remote base URL; replay succeeds and the mock
  provider receives no additional request.
- `live_provider_still_requires_an_api_key` (H2): the live path still fails
  closed with a missing-key error.
- `project_pricing_cannot_satisfy_the_max_cost_requirement` (H1): a trusted
  project pricing entry is ignored, and `--max-cost` fails with the documented
  missing-pricing error before authentication.
- `json_record_keeps_stdout_pure_jsonl_and_reports_success_on_stderr` (H3):
  every stdout line parses as JSON with `v = 1`; recorder success text appears
  only on stderr.
- `json_record_failure_is_visible_on_stderr` (H3): a recorder write failure is
  reported on stderr while stdout remains valid JSONL.
- `human_readable_output_still_works` (H3 regression): normal text output and
  the session summary remain functional.

The existing H1 unit test
`trusted_project_cannot_override_user_provider_credentials_pricing_or_redaction`
continues to cover provider/endpoint/model/`api_key_env`/preset/pricing/
redaction/default-provider invariants. Existing cassette tests continue to
cover request matching, exhaustion, divergence, and SSE parsing.

### `crates/raven-tools/src/write_support.rs` (3 tests)

- `atomic_create_creates_a_new_file_and_cleans_up_the_temp_file` (M1)
- `atomic_create_refuses_an_existing_target_and_preserves_it` (M1)
- `atomic_write_still_replaces_an_existing_file` (M1 edit regression)

### `crates/raven-tools/src/create_file.rs` (1 test)

- `a_target_appearing_during_approval_is_preserved` (M1 race): a custom
  approver creates the target during approval; the create fails and the raced
  file remains byte-for-byte unchanged with no temporary files left behind.

## Failures, baseline, and skipped checks

- **New branch failure:** `cargo fmt --all -- --check` fails in production files
  added/modified by the branch (see detail above).
- **Validation-test failures:** none.
- **Skipped/unavailable:** non-Windows execution (Unix `hard_link` create path)
  was not run on this Windows host. No GitHub Actions, paid CI, paid APIs, or
  external services were used.
- No dependency, `Cargo.lock`, architecture, `main`, merge, or PR changes were
  made.

## Verdict

**FAIL** — all focused regressions, workspace tests, Clippy, release build, E2E,
and whitespace checks pass, but the required formatting gate fails in production
files committed on the branch. Fixing those files is outside the test-only scope
of this validation task, so the branch requires remediation before it can be
considered fully validated.
