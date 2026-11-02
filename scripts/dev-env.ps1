#Requires -Version 5.1
<#
.SYNOPSIS
    Configures PATH and LIB for building Raven CLI with the MSVC toolchain.

.DESCRIPTION
    Finds the Rust toolchain, Visual Studio C++ build tools, and the Windows
    SDK libraries. If the Windows SDK is not installed, -DownloadSdk fetches the
    Microsoft.Windows.SDK.CPP.x64 NuGet package into
    %LOCALAPPDATA%\raven-sdk as a fallback (no admin rights required).

    This script must be dot-sourced so the environment changes remain in the
    current shell:

        . .\scripts\dev-env.ps1
        . .\scripts\dev-env.ps1 -DownloadSdk
        . .\scripts\dev-env.ps1 -SdkPath C:\path\to\extracted\sdk

.PARAMETER DownloadSdk
    Download and extract the Windows SDK libraries from NuGet if no installed
    SDK is found.

.PARAMETER SdkPath
    Path to an extracted Microsoft.Windows.SDK.CPP.x64 package (the directory
    that contains c\um\x64 and c\ucrt\x64). Skips SDK auto-detection.

.EXAMPLE
    . .\scripts\dev-env.ps1
    cargo build --release

.NOTES
    Installing "Desktop development with C++" plus the Windows 10/11 SDK from
    the Visual Studio Installer is the supported setup. The NuGet fallback is a
    convenience for locked-down machines; it does not install headers, tools,
    or a debugger.
#>
[CmdletBinding()]
param(
    [switch]$DownloadSdk,
    [string]$SdkPath
)

$previousErrorActionPreference = $ErrorActionPreference
$ErrorActionPreference = 'Stop'

function Find-RustBin {
    $candidates = @(
        (Join-Path $env:USERPROFILE '.rustup\toolchains\stable-x86_64-pc-windows-msvc\bin'),
        (Join-Path $env:USERPROFILE '.cargo\bin')
    )

    foreach ($candidate in $candidates) {
        if (Test-Path (Join-Path $candidate 'cargo.exe')) {
            return $candidate
        }
    }

    $cargo = Get-Command cargo -ErrorAction SilentlyContinue
    if ($cargo) {
        return Split-Path -Parent $cargo.Source
    }

    throw "cargo.exe was not found. Install Rust from https://rustup.rs (MSVC toolchain) and retry."
}

function Find-VsInstall {
    $vswhere = Join-Path ([Environment]::GetFolderPath('ProgramFilesX86')) 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) {
        return $null
    }

    $path = & $vswhere -latest -products * `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -property installationPath 2>$null
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($path)) {
        return $null
    }
    return $path.Trim()
}

function Find-MsvcLibDirs {
    param([string]$VsInstall)

    $msvcRoot = Join-Path $VsInstall 'VC\Tools\MSVC'
    if (-not (Test-Path $msvcRoot)) {
        throw "Visual Studio C++ tools were not found under '$msvcRoot'."
    }

    $msvc = Get-ChildItem $msvcRoot -Directory |
        Sort-Object { [version]($_.Name.Split('-')[0]) } -Descending |
        Select-Object -First 1

    $bin = Join-Path $msvc.FullName 'bin\Hostx64\x64'
    $lib = Join-Path $msvc.FullName 'lib\x64'
    if (-not (Test-Path (Join-Path $bin 'link.exe'))) {
        throw "link.exe was not found under '$bin'. Install the MSVC x64 build tools."
    }
    if (-not (Test-Path (Join-Path $lib 'libcpmt.lib'))) {
        throw "The MSVC x64 C++ libraries were not found under '$lib'."
    }

    return [pscustomobject]@{ Bin = $bin; Lib = $lib }
}

function Find-WindowsSdkDirs {
    $kitsLib = Join-Path ([Environment]::GetFolderPath('ProgramFilesX86')) 'Windows Kits\10\Lib'
    if (-not (Test-Path $kitsLib)) {
        return $null
    }

    foreach ($version in (Get-ChildItem $kitsLib -Directory | Sort-Object Name -Descending)) {
        $um = Join-Path $version.FullName 'um\x64'
        $ucrt = Join-Path $version.FullName 'ucrt\x64'
        if ((Test-Path (Join-Path $um 'kernel32.lib')) -and
            (Test-Path (Join-Path $ucrt 'ucrt.lib'))) {
            return [pscustomobject]@{ Um = $um; Ucrt = $ucrt; Version = $version.Name }
        }
    }
    return $null
}

function Find-NuGetSdk {
    param([string]$Root)

    if (-not (Test-Path $Root)) {
        return $null
    }

    $um = Join-Path $Root 'c\um\x64'
    $ucrt = Join-Path $Root 'c\ucrt\x64'
    if ((Test-Path (Join-Path $um 'kernel32.lib')) -and
        (Test-Path (Join-Path $ucrt 'ucrt.lib'))) {
        return [pscustomobject]@{ Um = $um; Ucrt = $ucrt; Version = (Split-Path -Leaf $Root) }
    }
    return $null
}

function Install-NuGetSdk {
    $root = Join-Path $env:LOCALAPPDATA 'raven-sdk'
    New-Item -ItemType Directory -Force $root | Out-Null

    $index = Invoke-RestMethod 'https://api.nuget.org/v3-flatcontainer/microsoft.windows.sdk.cpp.x64/index.json'
    $version = $index.versions |
        Where-Object { $_ -notmatch '-' } |
        Select-Object -Last 1
    if (-not $version) {
        throw "Could not find a stable Microsoft.Windows.SDK.CPP.x64 package on nuget.org."
    }

    $extract = Join-Path $root "sdk-$version"
    $existing = Find-NuGetSdk -Root $extract
    if ($existing) {
        return $existing
    }

    $nupkg = Join-Path $root "microsoft.windows.sdk.cpp.x64.$version.nupkg"
    if (-not (Test-Path $nupkg)) {
        Write-Host "Downloading Windows SDK libraries $version (this can take a few minutes)..." -ForegroundColor Cyan
        Invoke-WebRequest "https://api.nuget.org/v3-flatcontainer/microsoft.windows.sdk.cpp.x64/$version/microsoft.windows.sdk.cpp.x64.$version.nupkg" -OutFile $nupkg
    }

    Write-Host "Extracting Windows SDK libraries..." -ForegroundColor Cyan
    if (Test-Path $extract) {
        Remove-Item -Recurse -Force $extract
    }
    Expand-Archive -Path $nupkg -DestinationPath $extract -Force

    $sdk = Find-NuGetSdk -Root $extract
    if (-not $sdk) {
        throw "The downloaded package did not contain c\um\x64 and c\ucrt\x64."
    }
    return $sdk
}

try {
$rustBin = Find-RustBin
$vsInstall = Find-VsInstall
if (-not $vsInstall) {
    throw "Visual Studio C++ build tools were not found. Install 'Build Tools for Visual Studio' with the Desktop development with C++ workload."
}

$msvc = Find-MsvcLibDirs -VsInstall $vsInstall

$sdk = $null
if ($SdkPath) {
    $sdk = Find-NuGetSdk -Root $SdkPath
    if (-not $sdk) {
        throw "SdkPath '$SdkPath' does not contain c\um\x64\kernel32.lib and c\ucrt\x64\ucrt.lib."
    }
} else {
    $sdk = Find-WindowsSdkDirs
    if (-not $sdk -and $DownloadSdk) {
        $sdk = Install-NuGetSdk
    }
}

if (-not $sdk) {
    throw "No Windows SDK libraries found. Install the Windows 10/11 SDK from the Visual Studio Installer, or run: . .\scripts\dev-env.ps1 -DownloadSdk"
}

$env:PATH = "$rustBin;$($msvc.Bin);$env:PATH"
$env:LIB = "$($sdk.Um);$($sdk.Ucrt);$($msvc.Lib)"

Write-Host "Rust:     $rustBin" -ForegroundColor Green
Write-Host "MSVC:     $($msvc.Bin)" -ForegroundColor Green
Write-Host "SDK:      $($sdk.Um)" -ForegroundColor Green
Write-Host "Ready. Run: cargo build --release" -ForegroundColor Green
} finally {
    # Dot-sourcing shares this variable with the caller's shell. Leave the
    # caller's native-command error handling as it was; otherwise cargo's stderr
    # output can become a terminating error in PowerShell 5.1.
    $ErrorActionPreference = $previousErrorActionPreference
}
