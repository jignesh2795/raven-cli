# Revalidation evidence: Raven security audit batch 1

## Revision identity

| Item | Value |
| --- | --- |
| Repository | `https://github.com/jignesh2795/raven-cli` |
| Branch | `fix/audit-h1-h3-create-only-v1` |
| Validated implementation SHA | `f36a4174c7f6ca67f86d57f3ddd641a58b4627d0` |
| Remote branch HEAD at validation | `f36a4174c7f6ca67f86d57f3ddd641a58b4627d0` |
| Local branch HEAD at validation | `f36a4174c7f6ca67f86d57f3ddd641a58b4627d0` |
| Base | `5852d53dbd37a8b6171a430860c350356abc0c9a` |
| `main` (local and remote) | `5852d53dbd37a8b6171a430860c350356abc0c9a` |
| Base ancestry | Preserved (`git merge-base --is-ancestor`, exit `0`) |
| Working tree before validation | Clean |

The local branch was fast-forwarded from the previous evidence commit
`0424aed7832c68d2dccbd310e3ff6866f99d6f4a` to the exact target. No reset,
rebase, force-push, merge, or PR was performed. The only change published by
this revalidation is this evidence file.

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

## Formatting gate — FAIL

Command:

```powershell
cargo fmt --all -- --check
```

Exit code: **1**

The `trust.rs` formatting issue from the previous validation is fixed. The
remaining failure is confined to `crates/raven-cli/src/config.rs`:

- `config.rs:588` — `let local = config` / `.resolve_provider(...)` / `.unwrap()`
  is split across three lines, but rustfmt joins it into one line.
- `config.rs:593` — the `assert_eq!(local.api_key_env, None, ...)` call is on
  one line, but rustfmt splits it into multiple lines.
- `config.rs:595` — `let pricing = config.pricing.get("user-model").unwrap();`
  is split across lines, but rustfmt joins it into one line.
- `config.rs:603` — the `assert!(config.effective_redact(), ...)` call is on one
  line, but rustfmt splits it into multiple lines.

Complete formatter diff:

```diff
Diff in \\?\E:\Windsurfbased\raven-cli\crates\raven-cli\src\config.rs:585:
         assert_eq!(openai.model, "gpt-4.1");
         assert_eq!(openai.api_key_env.as_deref(), Some("USER_OPENAI_API_KEY"));

-        let local = config
-            .resolve_provider("local", None, None, None)
-            .unwrap();
+        let local = config.resolve_provider("local", None, None, None).unwrap();
         assert_eq!(local.base_url, "http://127.0.0.1:8780/v1");
         assert_eq!(local.model, "local-model");
-        assert_eq!(local.api_key_env, None, "project credentials must be ignored");
+        assert_eq!(
+            local.api_key_env, None,
+            "project credentials must be ignored"
+        );

-        let pricing = config
-            .pricing
-            .get("user-model")
-            .unwrap();
+        let pricing = config.pricing.get("user-model").unwrap();
         assert_eq!(pricing.input_per_mtok, 5.0);
         assert_eq!(pricing.output_per_mtok, 7.0);
         assert_eq!(pricing.cached_input_per_mtok, 1.0);
Diff in \\?\E:\Windsurfbased\raven-cli\crates\raven-cli\src\config.rs:602:
         assert!(!config.pricing.contains_key("project-model"));
-        assert!(config.effective_redact(), "project redact=false must be ignored");
+        assert!(
+            config.effective_redact(),
+            "project redact=false must be ignored"
+        );
     }
```

The remediation commit `f36a417` split expressions that rustfmt keeps on one
line and left long assertions that rustfmt splits. This is a genuine formatting
failure at the validated SHA. Fixing it requires running rustfmt on production
code, which is outside the scope of this validation task, so it was recorded
rather than changed.

## Focused regression tests — all pass

| Command | Result | Exit |
| --- | --- | --- |
| `cargo test -p raven-cli --test cli_regressions` | 6 passed, 0 failed | 0 |
| `cargo test -p raven-cli trusted_project_cannot_override_user_provider_credentials_pricing_or_redaction` | 1 passed, 0 failed | 0 |
| `cargo test -p raven-sandbox create_only` | 2 passed, 0 failed | 0 |
| `cargo test -p raven-tools` | 14 passed, 0 failed | 0 |

These cover the H1 trust boundary, H2 offline replay/no-key and live-key
requirements, H3 JSONL/stderr behavior, and M1 create-only/race/temp-cleanup and
edit replacement invariants. No test code was changed during this revalidation.

## Full validation matrix

| Command | Result | Exit |
| --- | --- | --- |
| `cargo fmt --all -- --check` | **FAIL** — see above | **1** |
| `cargo test --workspace` | 89 passed, 0 failed | 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | Clean | 0 |
| `cargo build --workspace --release` | Clean | 0 |
| `powershell -ExecutionPolicy Bypass -File scripts/e2e.ps1` | `E2E OK` (session `20261106-153008-38411d`) | 0 |
| `git diff --check` | Clean | 0 |

Workspace test breakdown: raven-cli unit `9`, canary `5`, CLI regressions `6`,
raven-core `36`, raven-provider `13`, raven-sandbox `4`, raven-store `2`,
raven-tools `14` (89 total). Doc-tests: 0.

The E2E run exercised `read_file`, `edit_file`, `create_file`, approval, audit
logging, and session reporting against the local mock provider (4 turns).

## Additional verification

- Replay without credentials/network: covered by
  `replay_works_without_an_api_key_and_does_not_contact_the_provider`; passed.
- Live execution still requires credentials: covered by
  `live_provider_still_requires_an_api_key`; passed.
- JSONL stdout purity and stderr recorder diagnostics: covered by the two
  `json_record_*` tests; passed.
- Project configuration cannot override user-owned security settings and
  built-in presets remain protected: covered by the focused H1 unit test and
  the CLI `--max-cost` project-pricing test; passed.
- Windows create-only preserves unexpected files and cleans temporary files:
  native sandbox tests and the tool-level approval-race test passed.
- Existing edit replacement continues to work: `raven-tools` edit tests and the
  E2E edit step passed.

## Windows-specific verification

Validation ran natively on Windows MSVC. `crates/raven-sandbox/src/atomic.rs`
calls `MoveFileExW` with `MOVEFILE_WRITE_THROUGH` only; the `REPLACE_EXISTING`
flag is not passed, so an unexpected target causes an error instead of being
replaced. The native create-only tests and the approval-race test passed.

The Unix `hard_link` create path was source-reviewed but not executed on this
Windows host. No Unix execution is claimed.

## Unavailable / skipped checks

- Non-Windows execution was not available on this host.
- No GitHub Actions, paid CI, paid APIs, or external services were used.

## Verdict

**FAIL** — every functional, Clippy, release-build, E2E, and whitespace gate
passes, but the required formatting gate fails in production file
`crates/raven-cli/src/config.rs`. The formatting remediation commit is
incomplete and must be corrected before batch 1 can receive a PASS verdict.
