# Building Raven CLI on Windows

Raven targets `x86_64-pc-windows-msvc`. A GNU toolchain or WSL is not
supported for the Windows build.

## Supported setup

1. **Rust (MSVC toolchain)** — install from <https://rustup.rs>. The project is
   tested with Rust 1.91.1 and declares `rust-version = "1.80"`.
2. **Visual Studio Build Tools 2019 or later** with:
   - Desktop development with C++ / MSVC x64 build tools
   - Windows 10 or Windows 11 SDK (x64 libraries)

Using the Visual Studio Installer is the supported path. Validate the install:

```powershell
where.exe link.exe
Test-Path "C:\Program Files (x86)\Windows Kits\10\Lib"
```

If `link.exe` is not found, open a **Developer PowerShell for VS**, or run the
setup script described below.

## Configure the current shell

The repository includes `scripts/dev-env.ps1`, which locates the Rust
toolchain, MSVC, and the Windows SDK and sets `PATH`/`LIB` for the shell.

```powershell
. .\scripts\dev-env.ps1
cargo build --release
```

The binary is written to `target\release\raven.exe`.

> The script must be dot-sourced (note the leading `.`), otherwise the
> environment changes are lost when the script exits.

## When the Windows SDK is missing

Some Visual Studio Build Tools installs include the MSVC compiler but no
Windows SDK. The linker then fails with:

```text
LNK1181: cannot open input file 'kernel32.lib'
```

Install the Windows SDK from the Visual Studio Installer, or use the
no-admin NuGet fallback:

```powershell
. .\scripts\dev-env.ps1 -DownloadSdk
cargo build --release
```

The fallback downloads `Microsoft.Windows.SDK.CPP.x64` from nuget.org into
`%LOCALAPPDATA%\raven-sdk` and sets `LIB` to its `c\um\x64` and `c\ucrt\x64`
directories. It provides link libraries only; it is not a replacement for a
full SDK if you later need Windows headers, `rc.exe`, or debugging tools.

The download persists. Later shells auto-detect it, and plain `.\scripts\dev-env.ps1` works without arguments:

```powershell
. .\scripts\dev-env.ps1
```

Resolution order: installed SDK from the Visual Studio Installer first, then
the persisted NuGet SDK. `-SdkPath` is only needed to point at a package
extracted somewhere else, such as a `sdk-<version>` directory extracted by hand
into `%TEMP%`.

## Rust not on PATH

A minimal rustup install may place `cargo.exe` under
`%USERPROFILE%\.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin` without a
`%USERPROFILE%\.cargo\bin` shim. `dev-env.ps1` detects that location. To fix it
permanently, run `rustup default stable-x86_64-pc-windows-msvc` once from a
working rustup installation, or add the toolchain `bin` directory to `PATH`.

## Common commands

```powershell
. .\scripts\dev-env.ps1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
powershell -ExecutionPolicy Bypass -File .\scripts\e2e.ps1
```

`cargo fmt` and `cargo clippy` require the `rustfmt` and `clippy` components:

```powershell
rustup component add rustfmt clippy
```

The end-to-end script requires Python 3. It starts
`scripts/mock_provider.py` on loopback, runs one read-only agent session, and
prints the recorded session. It does not call a real model provider.

## Troubleshooting

| Symptom | Cause | Fix |
| --- | --- | --- |
| `link.exe` not found | MSVC tools are not on `PATH` | Dot-source `scripts/dev-env.ps1` or use a Developer PowerShell |
| `LNK1181: cannot open input file 'kernel32.lib'` | Windows SDK missing | Install the SDK or use `-DownloadSdk` |
| `link.exe` exits with `LNK1104` for another `.lib` | `LIB` points at a partial SDK | Re-run the script without a stale `LIB`, or pass `-SdkPath` |
| `cargo` not recognized | Rust toolchain `bin` not on `PATH` | Dot-source the script; see above |
| `error: linker ... not found` from a non-MSVC target | Wrong toolchain | `rustup default stable-x86_64-pc-windows-msvc` |
