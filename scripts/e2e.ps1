# End-to-end smoke test for M0, using the local mock provider.
# Run from the repository root after `cargo build`.

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$temp = Join-Path $env:TEMP ("raven-e2e-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
New-Item -ItemType Directory -Force $temp | Out-Null
$config = Join-Path $temp "raven.toml"
$env:RAVEN_DATA_DIR = Join-Path $temp "data"

@"
default_provider = "mock"
max_turns = 4
output_cap = 65536

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

    & $raven --config $config run "Summarize the README"
    if ($LASTEXITCODE -ne 0) { throw "raven run failed with exit code $LASTEXITCODE" }

    & $raven --config $config sessions
    $session = (& $raven --config $config sessions --json | ConvertFrom-Json | Select-Object -First 1).id
    if (-not $session) { throw "no session was recorded" }

    & $raven --config $config show $session | Select-Object -First 20

    Write-Host "`nE2E OK (session $session)"
}
finally {
    if ($mock -and -not $mock.HasExited) { Stop-Process -Id $mock.Id -Force }
    Remove-Item -Recurse -Force $temp -ErrorAction SilentlyContinue
}
