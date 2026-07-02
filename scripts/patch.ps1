$ErrorActionPreference = "Stop"

function Invoke-Native {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Name,

        [string[]]$Arguments = @()
    )

    & $Name @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Name failed with exit code $LASTEXITCODE"
    }
}

$root = (git rev-parse --show-toplevel).Trim()
if ($LASTEXITCODE -ne 0) {
    throw "git rev-parse failed with exit code $LASTEXITCODE"
}

Set-Location $root

Invoke-Native "vergo" @("-project-root", $root, "-debug", "-update")

$packageJson = Get-Content -Raw -Path (Join-Path $root "package.json") | ConvertFrom-Json
$version = [string]$packageJson.version

Invoke-Native "sumry" @("-project-root", $root, "-debug", "-update")

if ($null -eq [Environment]::GetEnvironmentVariable("PATCH_BINDINGS_CMD")) {
    $bindingsCmd = "pnpm exec cargo run --example gen_bindings --features=gen_bindings"
} else {
    $bindingsCmd = $env:PATCH_BINDINGS_CMD
}

if ($bindingsCmd -ne "") {
    Invoke-Expression $bindingsCmd
    if ($LASTEXITCODE -ne 0) {
        throw "PATCH_BINDINGS_CMD failed with exit code $LASTEXITCODE"
    }
}

if ([string]::IsNullOrWhiteSpace($env:PATCH_FILES)) {
    $files = @(
        "package.json",
        "Cargo.lock",
        "src-tauri/Cargo.toml",
        "src-tauri/tauri.conf.json",
        "SUMRY.md",
        "archive/"
    )
} else {
    $files = $env:PATCH_FILES -split "\s+" | Where-Object { $_ -ne "" }
}

Invoke-Native "git" (@("add") + $files)
Invoke-Native "git" @("commit", "-m", "chore(updater): version bump $version")
