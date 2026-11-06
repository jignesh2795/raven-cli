# Security audit batch 2 validation evidence

## Revision identity

| Item | Value |
| --- | --- |
| Repository | `https://github.com/jignesh2795/raven-cli` |
| Branch | `fix/security-audit-batch-2-v1` |
| Validated implementation SHA | `f4b722a480de5ae7c186b12a214c53648f35daec` |
| Local branch HEAD at validation | `f4b722a480de5ae7c186b12a214c53648f35daec` |
| Remote branch HEAD at validation | `f4b722a480de5ae7c186b12a214c53648f35daec` |
| Base / `main` | `16b91d4197a8d232504eb16352fa50f94128dfcb` |
| Remote `main` at validation | `16b91d4197a8d232504eb16352fa50f94128dfcb` |
| Base ancestry | Preserved (`git merge-base --is-ancestor`, exit `0`) |
| Working tree before validation | Clean |
| `Cargo.lock` uncommitted changes | None (`git diff --exit-code -- Cargo.lock`, exit `0`) |

This clone has no local `main` ref. Its pre-existing local mainline ref is
`master`, which remains at the older Batch 1 main SHA
`5852d53dbd37a8b6171a430860c350356abc0c9a` and was not modified. The remote
mainline `origin/main` matches the Batch 2 base exactly. The local branch was
created by tracking `origin/fix/security-audit-batch-2-v1`; no reset, rebase,
force-push, or `main` modification was performed.

**The evidence commit is separate from the validated implementation SHA.** It is
a child commit containing only this document and does not alter the
implementation at `f4b722a480de5ae7c186b12a214c53648f35daec`.

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

The formatter reports formatting diffs in production files and then fails to
parse the integration test file because of invalid Rust Unicode escape
sequences. Both problems are at the validated implementation SHA and were not
modified during validation.

Failing formatter paths:

- `crates/raven-cli/src/auth.rs:207`
- `crates/raven-cli/src/doctor.rs:25`
- `crates/raven-cli/src/doctor.rs:96`
- `crates/raven-cli/src/sessions.rs:83`
- `crates/raven-cli/src/trust.rs:47`
- `crates/raven-cli/src/trust.rs:191`
- `crates/raven-cli/src/trust.rs:216`
- `crates/raven-cli/src/trust.rs:329`

Complete formatter diff:

```diff
Diff in \\?\E:\Windsurfbased\raven-cli\crates\raven-cli\src\auth.rs:207:
             "not a URL",
             "/relative/path/localhost",
         ] {
-            assert!(!is_local(url), "remote or malformed endpoint accepted: {url}");
+            assert!(
+                !is_local(url),
+                "remote or malformed endpoint accepted: {url}"
+            );
         }
     }
 }
Diff in \\?\E:\Windsurfbased\raven-cli\crates\raven-cli\src\doctor.rs:25:
         if path.exists() { "found" } else { "not found" }
     );
     println!("data:      {}", terminal_safe(&data.display().to_string()));
-    println!("workspace: {}", terminal_safe(&workspace.display().to_string()));
     println!(
+        "workspace: {}",
+        terminal_safe(&workspace.display().to_string())
+    );
+    println!(
         "policy:    {} (edits require approval unless --yes)",
         config.effective_policy().label()
     );
Diff in \\?\E:\Windsurfbased\raven-cli\crates\raven-cli\src\doctor.rs:96:
 fn describe(result: &raven_sandbox::ProbeResult) -> String {
     match result {
         raven_sandbox::ProbeResult::Available => "available".into(),
-        raven_sandbox::ProbeResult::Unavailable(reason) => format!("unavailable: {}", terminal_safe(reason)),
-        raven_sandbox::ProbeResult::Unknown(reason) => format!("unknown: {}", terminal_safe(reason)),
-        raven_sandbox::ProbeResult::Planned(reason) => format!("planned: {}", terminal_safe(reason)),
+        raven_sandbox::ProbeResult::Unavailable(reason) => {
+            format!("unavailable: {}", terminal_safe(reason))
+        }
+        raven_sandbox::ProbeResult::Unknown(reason) => {
+            format!("unknown: {}", terminal_safe(reason))
+        }
+        raven_sandbox::ProbeResult::Planned(reason) => {
+            format!("planned: {}", terminal_safe(reason))
+        }
     }
 }
-

 fn terminal_safe(input: &str) -> String {
     Redactor::default().redact(&sanitize_text(input))
Diff in \\?\E:\Windsurfbased\raven-cli\crates\raven-cli\src\sessions.rs:83:
         terminal_safe(&meta.model)
     );
     println!("policy:    {}", terminal_safe(&meta.policy));
-    println!("status:    {} · {} turns", terminal_safe(&meta.status), meta.turns);
+    println!(
+        "status:    {} · {} turns",
+        terminal_safe(&meta.status),
+        meta.turns
+    );
     println!(
         "usage:     {} in / {} out ({} cached)",
         meta.usage.input_tokens, meta.usage.output_tokens, meta.usage.cached_input_tokens
Diff in \\?\E:\Windsurfbased\raven-cli\crates\raven-cli\src\trust.rs:47:
         }
         let text = std::fs::read_to_string(&path)?;
         let records: Vec<TrustRecord> = serde_json::from_str(&text).map_err(|error| {
-            Error::Config(format!("invalid trust store '{}': {error}", terminal_safe(&path.display().to_string())))
+            Error::Config(format!(
+                "invalid trust store '{}': {error}",
+                terminal_safe(&path.display().to_string())
+            ))
         })?;
         Ok(Self { records })
     }
Diff in \\?\E:\Windsurfbased\raven-cli\crates\raven-cli\src\trust.rs:191:

     let path = project_config_path(&workspace);
     if !path.exists() {
-        println!("no project-local raven.toml at {}", terminal_safe(&path.display().to_string()));
+        println!(
+            "no project-local raven.toml at {}",
+            terminal_safe(&path.display().to_string())
+        );
         list_records(&data_dir)?;
         return Ok(());
     }
Diff in \\?\E:\Windsurfbased\raven-cli\crates\raven-cli\src\trust.rs:216:
     if !grant {
         // Neither grant nor revoke: report the current state and stop. Never
         // prompt for a trust decision the user did not ask for.
-        println!("{} is NOT trusted", terminal_safe(&workspace.display().to_string()));
+        println!(
+            "{} is NOT trusted",
+            terminal_safe(&workspace.display().to_string())
+        );
         println!("  project-local raven.toml is being ignored. It would change:");
         show_project_config(&project);
         println!("\n  run `raven trust --grant` to review and trust it");
Diff in \\?\E:\Windsurfbased\raven-cli\crates\raven-cli\src\trust.rs:329:
         println!(
             "  {} · {} · since {}",
             terminal_safe(&record.workspace),
-            terminal_safe(record.config_sha256.get(..16).unwrap_or(&record.config_sha256)),
+            terminal_safe(
+                record
+                    .config_sha256
+                    .get(..16)
+                    .unwrap_or(&record.config_sha256)
+            ),
             terminal_safe(&record.trusted_at)
         );
     }
```

The formatter also reports nine `incorrect unicode escape sequence` errors while
parsing `crates/raven-cli/tests/cli_regressions.rs`:

- line `510`, column `23`: `"\u001b]0;owned\u0007C:/workspace"`
- line `510`, column `37`: same string, second escape
- line `511`, column `30`: `"provider\u001b[31m"`
- line `512`, column `24`: `"model\u001b[0m"`
- line `514`, column `29`: `"completed\u001b[31m"`
- line `536`, column `10`: `"\u001b[31mred summary\u001b[0m"`
- line `536`, column `31`: same string, second escape
- line `537`, column `41`: `"\u001b]0;hidden\u0007"`
- line `537`, column `56`: same string, second escape

Rust requires braces around Unicode escapes; these should be `\u{001b}` and
`\u{0007}`. This is a test-source defect at the exact implementation SHA. It was
not fixed because the task forbids modifying tests.

## Focused regression tests — all BLOCKED by compile failure

Required commands and results:

| Command | Result | Exit |
| --- | --- | --- |
| `cargo test -p raven-cli recognizes_only_exact_loopback_hosts --locked` | 0 tests run; compile error | 101 |
| `cargo test -p raven-cli --test cli_regressions trust_review_sanitizes_untrusted_project_config_output --locked` | 0 tests run; compile error | 101 |
| `cargo test -p raven-cli --test cli_regressions doctor_sanitizes_provider_config_before_terminal_display --locked` | 0 tests run; compile error | 101 |
| `cargo test -p raven-cli --test cli_regressions show_and_sessions_sanitize_persistent_metadata_and_event_summaries --locked` | 0 tests run; compile error | 101 |
| `cargo test -p raven-cli --test cli_regressions --locked` | 0 tests run; compile error | 101 |

Every command failed with:

```text
error: could not compile `raven-cli` (test "cli_regressions") due to 9 previous errors
```

The compile failure is in the integration test target only. The unit-test target
still compiles and passes, as recorded in the diagnostic runs below.

Diagnostic runs added for diagnosis only:

| Command | Result | Exit |
| --- | --- | --- |
| `cargo test -p raven-cli --bin raven recognizes_only_exact_loopback_hosts --locked` | 1 passed | 0 |
| `cargo test -p raven-cli --bin raven --locked` | 10 passed | 0 |

## Full validation matrix

| Command | Result | Exit |
| --- | --- | --- |
| `cargo fmt --all -- --check` | FAIL | 1 |
| `cargo test --workspace --locked` | FAIL: `could not compile raven-cli (test "cli_regressions") due to 9 previous errors` | 101 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | FAIL: same compile error | 101 |
| `cargo build --workspace --release --locked` | Clean | 0 |
| `powershell -ExecutionPolicy Bypass -File scripts/e2e.ps1` | `E2E OK` (session `20261106-161153-a5c084`) | 0 |
| `git diff --check` | Clean | 0 |
| `git diff --exit-code -- Cargo.lock` | Clean | 0 |

No workspace tests ran because compilation stopped. Release build and E2E pass
because they do not compile the integration-test target.

## Behavior and security review

### Issue #2 — local endpoint detection

Source review at `crates/raven-cli/src/auth.rs` confirms the implemented
invariants:

- `is_local` parses the URL with `url::Url::parse` and returns `false` on parse
  failure.
- Only `http` and `https` are eligible.
- Host matching is exact: `localhost` is accepted case-insensitively as a domain,
  IPv4 loopback uses `Ipv4Addr::is_loopback`, IPv6 loopback uses
  `Ipv6Addr::is_loopback`, and a missing host is rejected.
- `0.0.0.0`, unspecified IPv6 `[::]`, malformed URLs, unsupported schemes, remote
  hostnames, and URL paths containing `localhost` or `127.0.0.1` are not accepted.

The issue #2 unit test
`auth::tests::recognizes_only_exact_loopback_hosts` passed in the diagnostic run
with `--bin raven`. The required `cargo test -p raven-cli` commands still exit
`101` because Cargo compiles the broken integration test target before applying
the test filter.

### Issue #3 — terminal output safety

Source review confirms sanitization is applied at the terminal boundaries:

- `raven trust` uses `terminal_safe` for provider names, endpoints, models,
  `api_key_env`, workspace paths, hashes, and trusted timestamps
  (`crates/raven-cli/src/trust.rs`).
- `terminal_safe_endpoint` parses the endpoint and removes username, password,
  query, and fragment. If any sensitive part was removed, it displays
  `[REDACTED URL USERINFO/QUERY]` (`trust.rs:293`).
- `raven sessions` and `raven show` sanitize session metadata, event summaries,
  and displayed event data (`crates/raven-cli/src/sessions.rs`).
- `raven doctor` sanitizes config paths, provider names and endpoints, and
  sandbox probe reasons (`crates/raven-cli/src/doctor.rs`).
- `raven auth` sanitizes provider labels and credential command output
  (`crates/raven-cli/src/auth.rs`).
- Human-readable model text, tool names/results, notices, approval summaries,
  and diffs pass through `sanitize_text` (`render.rs`, `approve.rs`). The
  approval path relies on the already-redacted `ApprovalRequest` and strips
  terminal controls without replacing configured redaction.
- JSON/JSONL emission continues to serialize through `serde_json`, preserving
  valid JSON and JSON escaping.

The CLI-level Issue #3 regressions
`trust_review_sanitizes_untrusted_project_config_output`,
`doctor_sanitizes_provider_config_before_terminal_display`, and
`show_and_sessions_sanitize_persistent_metadata_and_event_summaries` could not
be executed because the integration test target fails to compile. Issue #3 is
therefore only source-verified at this validation, not runtime-validated through
the required CLI regressions.

No real credentials, external provider calls, paid services, or paid CI were
used.

## Skipped or unavailable checks

- Non-Windows runtime validation was not performed; this is a native Windows
  validation only.
- The required CLI regressions were skipped by the compiler because of the
  invalid Unicode escapes in `crates/raven-cli/tests/cli_regressions.rs`.
- Full workspace tests and Clippy were skipped by the compiler for the same
  reason.

## Verdict

**FAIL** — The exact implementation SHA fails the required primary formatting
gate and cannot compile the `raven-cli` integration test target. The production
release build and E2E pass, and the issue #2 unit test passes when bypassing the
broken integration target, but those results do not allow a PASS verdict because
the required focused CLI regressions, workspace tests, and Clippy all exit
`101`.

Remediation required before a new PASS attempt:

1. Replace invalid Rust Unicode escapes in
   `crates/raven-cli/tests/cli_regressions.rs` with braced forms, for example
   `\u{001b}` and `\u{0007}`.
2. Apply the exact rustfmt output shown above to `auth.rs`, `doctor.rs`,
   `sessions.rs`, and `trust.rs`.
3. Republish the implementation SHA and rerun the same validation matrix.
