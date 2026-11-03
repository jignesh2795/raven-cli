# End-to-end smoke test for M1, using the local mock provider.
# Exercises read_file, edit_file, create_file, approval (--yes), and the audit log.
# Run from the repository root after `cargo build`.

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$temp = Join-Path $env:TEMP ("raven-e2e-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
New-Item -ItemType Directory -Force $temp | Out-Null
$config = Join-Path $temp "raven.toml"
$workspace = Join-Path $temp "workspace"
$env:RAVEN_DATA_DIR = Join-Path $temp "data"

New-Item -ItemType Directory -Force $workspace | Out-Null
New-Item -ItemType Directory -Force (Join-Path $workspace "notes") | Out-Null
Set-Content -Path (Join-Path $workspace "sample.txt") -Value "line one`nline two" -NoNewline -Encoding utf8

@"
default_provider = "mock"
max_turns = 6
output_cap = 65536
policy = "workspace-write"

[providers.mock]
base_url = "http://127.0.0.1:8765/v1"
model = "mock-model"
stream_usage = true

[pricing."mock-model"]
input_per_mtok = 1.0
output_per_mtok = 2.0
cached_input_per_mtok = 0.25
"@ | Set-Content -Encoding utf8 $config

$mock = Start-Process -FilePath "python" `
    -ArgumentList @((Join-Path $root "scripts\mock_provider.py"), "--port", "8765") `
    -PassThru -WindowStyle Hidden

try {
    Start-Sleep -Milliseconds 700
    $raven = Join-Path $root "target\debug\raven.exe"
    if (-not (Test-Path $raven)) {
        $raven = Join-Path $root "target\release\raven.exe"
    }
    if (-not (Test-Path $raven)) {
        throw "raven.exe not found. Run 'cargo build' (or 'cargo build --release') first."
    }

    & $raven --config $config run --workspace $workspace --yes "Edit the sample file and create a note"
    if ($LASTEXITCODE -ne 0) { throw "raven run failed with exit code $LASTEXITCODE" }

    $sample = Get-Content (Join-Path $workspace "sample.txt") -Raw
    if ($sample -notmatch "line one \(edited\)") {
        throw "edit_file did not update sample.txt (content: $sample)"
    }
    if ($sample -notmatch "line two") {
        throw "edit_file lost unrelated content in sample.txt"
    }

    $created = Join-Path $workspace "notes\created.txt"
    if (-not (Test-Path $created)) { throw "create_file did not create notes/created.txt" }
    if ((Get-Content $created -Raw) -notmatch "created by the e2e fixture") {
        throw "created file has unexpected content"
    }

    & $raven --config $config sessions
    $session = (& $raven --config $config sessions --json | ConvertFrom-Json | Select-Object -First 1).id
    if (-not $session) { throw "no session was recorded" }

    & $raven --config $config show $session | Select-Object -First 30

    $log = Get-Content (Join-Path $env:RAVEN_DATA_DIR "sessions\$session\events.jsonl") -Raw
    if ($log -notmatch '"edit_applied"') { throw "edit_applied event missing from the audit log" }
    if ($log -notmatch '"before_sha256"') { throw "edit hashes missing from the audit log" }

    Write-Host "`nE2E OK (session $session)"
}
finally {
    if ($mock -and -not $mock.HasExited) { Stop-Process -Id $mock.Id -Force }
    Remove-Item -Recurse -Force $temp -ErrorAction SilentlyContinue
}
