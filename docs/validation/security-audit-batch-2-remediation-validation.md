# Raven Batch 2 remediation validation evidence

## Revision identity

| Item | Value |
| --- | --- |
| Repository | `https://github.com/jignesh2795/raven-cli` |
| Branch | `fix/security-audit-batch-2-v1` |
| Validated implementation SHA | `d28db607c2e1d0bdacadfc2f2343f509e4622c0a` |
| Local HEAD at validation | `d28db607c2e1d0bdacadfc2f2343f509e4622c0a` |
| Remote branch HEAD at validation | `d28db607c2e1d0bdacadfc2f2343f509e4622c0a` |
| Base | `16b91d4197a8d232504eb16352fa50f94128dfcb` |
| Remote `origin/main` at validation | `16b91d4197a8d232504eb16352fa50f94128dfcb` |
| Base ancestry | Preserved (`git merge-base --is-ancestor`, exit `0`) |
| Working tree before validation | Clean |
| `Cargo.lock` uncommitted changes | None (`git diff --exit-code -- Cargo.lock`, exit `0`) |

The local branch was fast-forwarded from the previous Batch 2 evidence commit
`c4120d732324661d8d0dcd39fdccd1e676cefa7e` to the exact target SHA. No reset,
rebase, force-push, merge, PR, or `main` change was performed.

This clone still has no local `main` ref. The older local `master` ref remains
at `5852d53dbd37a8b6171a430860c350356abc0c9a` from the Batch 1 history and was
not used or modified as `origin/main`. The authoritative `origin/main` value
matches the Batch 2 base exactly.

**The evidence commit is separate from the validated implementation SHA.** It is
a child commit containing only this document and does not alter the implementation
at `d28db607c2e1d0bdacadfc2f2343f509e4622c0a`.

## Environment

| Item | Value |
| --- | --- |
| OS | Microsoft Windows 10 Pro, version `10.0.19045`, build `19045` |
| Architecture | 64-bit |
| Rust | `rustc 1.99.1 (58cf8f9c3 2026-10-13)` |
| Cargo | `cargo 1.99.1 (5f94df478 2026-08-27)` |
| rustfmt | `rustfmt 1.10.0-stable (58cf8f9c35 2026-10-13)` |
| Active toolchain | `stable-x86_64-pc-windows-msvc` (default) |
| Host / target triple | `x86_64-pc-windows-msvc` |
| MSVC tools | `14.29.30133` (`Hostx64\x64`) |
| Windows SDK | `10.0.28000.2957` (NuGet fallback under `%LOCALAPPDATA%\raven-sdk`) |

All Cargo commands were run after dot-sourcing `scripts/dev-env.ps1`.

## Formatting gate — FAIL

Command:

```powershell
cargo fmt --all -- --check
```

Exit code: **1**

Complete formatter diff:

```diff
Diff in \\?\E:\Windsurfbased\raven-cli\crates\raven-cli\tests\cli_regressions.rs:525:
     });
     std::fs::write(
         session_dir.join("meta.json"),
-        serde_json::to_vec(&meta).unwrap()
+        serde_json::to_vec(&meta).unwrap(),
     )
     .unwrap();
```

The formatter hunk is reported at line `525`; the affected expression is on line
`528` in the current source. No parser/compiler diagnostics remain in the
formatter output. No files were modified during validation and no write-mode
formatting was run.

## Focused regression tests

Every command was run independently. The Unicode-escape compile failure from the
previous Batch 2 validation is gone. A different, genuine runtime regression
failure remains in the trust-review test.

| Command | Result | Exit |
| --- | --- | --- |
| `cargo test -p raven-cli recognizes_only_exact_loopback_hosts --locked` | 1 passed, 0 failed | 0 |
| `cargo test -p raven-cli --test cli_regressions trust_review_sanitizes_untrusted_project_config_output --locked` | 1 failed, 0 passed | 101 |
| `cargo test -p raven-cli --test cli_regressions doctor_sanitizes_provider_config_before_terminal_display --locked` | 1 passed, 0 failed | 0 |
| `cargo test -p raven-cli --test cli_regressions show_and_sessions_sanitize_persistent_metadata_and_event_summaries --locked` | 1 passed, 0 failed | 0 |
| `cargo test -p raven-cli --test cli_regressions --locked` | 8 passed, 1 failed | 101 |

The focused loopback run also included the other test targets with zero filtered
matches. The loopback unit-test result was:

```text
running 1 test
test auth::tests::recognizes_only_exact_loopback_hosts ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out; finished in 0.00s
```

The trust-review failure is:

```text
thread 'trust_review_sanitizes_untrusted_project_config_output' panicked at crates\raven-cli\tests\cli_regressions.rs:448:5:
URL credentials were not visibly redacted: C:\Users\Jignesh Bhirud\AppData\Local\Temp\.tmppg9CKn\workspace is NOT trusted
  project-local raven.toml is being ignored. It would change:
  - default_provider: local (ignored; user/CLI-owned)
  - providers (custom entries / unset non-credential fields only):
      local: base_url=https://evil.example/ [REDACTED URL USERINFO/QUERY] model=mockMODEL api_key_env=AWS_SECRET_ACCESS_KEY (ignored; user-owned)
  - policy: (unset)
  - max_turns: (unset)
  - max_cost_usd: (unset)
  - output_cap: (unset)
  - temperature: (unset)
  - max_tokens: (unset)
  - pricing: 0 project entries ignored (user-owned)
  - redact: (unset) (project value ignored; user-owned setting only)

  run `raven trust --grant` to review and trust it

no trusted workspaces
```

The failing assertion is:

```rust
assert!(
    stdout.contains("[REDACTED]"),
    "URL credentials were not visibly redacted: {stdout}"
);
```

The trust review actually printed `[REDACTED URL USERINFO/QUERY]`, which is more
specific than `[REDACTED]` and does not contain the exact assertion substring.
The other safety assertions in the same test did not fail: no terminal escape,
no `password123`, no `user:`, and no `token=` were displayed.

## Full validation matrix

| Command | Result | Exit |
| --- | --- | --- |
| `cargo fmt --all -- --check` | FAIL | 1 |
| `cargo test --workspace --locked` | FAIL; default fail-fast stopped after `trust_review_sanitizes_untrusted_project_config_output` | 101 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Clean | 0 |
| `cargo build --workspace --release --locked` | Clean | 0 |
| `powershell -ExecutionPolicy Bypass -File scripts/e2e.ps1` | `E2E OK` (session `20261106-164225-188faf`) | 0 |
| `git diff --check` | Clean | 0 |
| `git diff --exit-code -- Cargo.lock` | Clean | 0 |

Because the required workspace test command uses Cargo’s default fail-fast
behavior, it stopped after the failing CLI regression target. A diagnostic run
with `cargo test --workspace --locked --no-fail-fast` was also executed to
capture complete counts. It also exited `101`:

```text
running 10 tests   test result: ok. 10 passed; 0 failed
running 5 tests    test result: ok. 5 passed; 0 failed
running 9 tests    test result: FAILED. 8 passed; 1 failed
running 36 tests   test result: ok. 36 passed; 0 failed
running 13 tests   test result: ok. 13 passed; 0 failed
running 4 tests    test result: ok. 4 passed; 0 failed
running 2 tests    test result: ok. 2 passed; 0 failed
running 14 tests   test result: ok. 14 passed; 0 failed
```

Complete workspace total from the diagnostic run: **93 tests, 92 passed,
1 failed**. The only failing test is
`trust_review_sanitizes_untrusted_project_config_output`.

The E2E run exercised `read_file`, `edit_file`, `create_file`, approval, audit
logging, and session reporting against the local mock provider.

## Unicode-escape remediation review

The remediation correctly separates Rust escape syntax from TOML escape syntax:

- Rust string literals in assertions and JSON fixtures use braced Rust escapes
  such as `\u{001b}` and `\u{0007}`, and they create actual control characters.
- TOML fixture strings remain raw Rust string literals containing TOML’s own
  `\u001b` and `\u0007` escape syntax.

Representative source lines:

- `crates/raven-cli/tests/cli_regressions.rs:420`:
  `base_url = "https://user:password123@evil.example/\u001b]0;owned\u0007?token=abcdefghijklmnopqrst123456"`
- `crates/raven-cli/tests/cli_regressions.rs:510`:
  `"workspace": "\u{001b}]0;owned\u{0007}C:/workspace"`
- `crates/raven-cli/tests/cli_regressions.rs:536`:
  `"\u{001b}[31mred summary\u{001b}[0m"`

The invalid `\u001b` Rust escapes and the subsequent compile/parser failure are
resolved. The remaining formatting diff is unrelated to escape validity.

## Issue #2 review — provider URL host parsing

Source review at the exact SHA confirms:

- `auth::is_local` parses the URL and returns `false` for malformed URLs.
- Only `http` and `https` schemes are eligible.
- Domain hosts must equal `localhost` case-insensitively.
- IPv4 hosts use `Ipv4Addr::is_loopback`.
- IPv6 hosts use `Ipv6Addr::is_loopback`.
- Missing hosts, non-loopback hosts, `0.0.0.0`, unspecified IPv6 `[::]`, and
  path/query lookalikes fail closed.

Representative source (`crates/raven-cli/src/auth.rs:166`):

```rust
pub fn is_local(base_url: &str) -> bool {
    let Ok(url) = url::Url::parse(base_url) else {
        return false;
    };
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }

    match url.host() {
        Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
        Some(url::Host::Ipv4(address)) => address.is_loopback(),
        Some(url::Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}
```

Runtime verification:

- `auth::tests::recognizes_only_exact_loopback_hosts` passed with exit `0`.

## Issue #3 review — terminal output safety

Source review confirms the current sanitization boundaries:

- `terminal_safe` in `trust.rs`, `sessions.rs`, and `doctor.rs` applies
  `Redactor::default().redact(&sanitize_text(input))`.
- `trust::terminal_safe_endpoint` parses the endpoint and removes username,
  password, query, and fragment. It appends `[REDACTED URL USERINFO/QUERY]`
  when those parts were removed.
- `render.rs` sanitizes and redacts human-readable model text, tool names,
  tool results, and notices before display.
- `approve.rs` sanitizes approval summaries and diffs before display. Redaction
  is applied upstream when constructing the `ApprovalRequest`; the approval path
  preserves that behavior while stripping terminal controls.
- `sessions.rs` sanitizes session metadata, event summaries, and displayed
  event data. `sessions --json` and `show --json` still serialize through
  `serde_json`.
- JSON and JSONL emission continues to serialize through `serde_json`,
  preserving valid JSON escaping.

Runtime verification:

- `doctor_sanitizes_provider_config_before_terminal_display` passed: no ESC
  escape, provider name `mockBAD` remains readable, endpoint
  `example.invalid` remains readable.
- `show_and_sessions_sanitize_persistent_metadata_and_event_summaries` passed:
  no ESC escape from persistent metadata/event data, sanitized event summary
  remains readable.
- `trust_review_sanitizes_untrusted_project_config_output` failed only on the
  exact substring `[REDACTED]`; the actual output showed
  `[REDACTED URL USERINFO/QUERY]` and did not leak escape, password, userinfo,
  or query content.

## Coverage limitations

- Native Windows MSVC validation only. No non-Windows runtime validation was
  performed and none is claimed.
- The required `cargo test --workspace --locked` stopped after the first failing
  target because Cargo’s default is fail-fast; complete counts came from an
  additional `--no-fail-fast` diagnostic run.
- No real credentials, live providers, paid APIs, paid CI, or GitHub Actions
  were used.

## Verdict

**FAIL** — The exact implementation SHA has two remaining required-gate failures:

1. `cargo fmt --all -- --check` fails on one missing trailing comma in
   `crates/raven-cli/tests/cli_regressions.rs`.
2. `trust_review_sanitizes_untrusted_project_config_output` fails because the
   test expects `[REDACTED]` while trust review emits
   `[REDACTED URL USERINFO/QUERY]`.

Clippy, release build, E2E, whitespace check, and lockfile cleanliness all pass,
but those do not override the failed formatting and functional regression gates.
A PASS verdict is not justified.
