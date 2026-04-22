#Requires -Version 5.1
<#
.SYNOPSIS
    Verify (default) or install (-CI) system deps for a Tauri 2 + pnpm project
    on Windows. Shared across projects via the utils submodule. Mirrors
    install_deps.sh for macOS/Linux. Per-project extras live in
    <repo-root>\.install_deps.ps1 (dot-sourced if present).

.PARAMETER CI
    Install missing dependencies non-interactively via winget and corepack.
    Without -CI, the script only checks and prints install hints.
#>

[CmdletBinding()]
param(
    [switch]$CI
)

$ErrorActionPreference = 'Stop'

$script:Root = (& git rev-parse --show-toplevel).Trim()
Set-Location -LiteralPath $script:Root

$script:Mode = if ($CI) { 'install' } else { 'verify' }

$script:Missing = [System.Collections.Generic.List[string]]::new()

function Write-Ok   ([string]$Name)                      { Write-Host "  [OK]   $Name" -ForegroundColor Green }
function Write-Miss ([string]$Name, [string]$Hint)       { Write-Host "  [MISS] $Name -- $Hint" -ForegroundColor Red }
function Write-Warn ([string]$Name, [string]$Hint)       { Write-Host "  [WARN] $Name -- $Hint" -ForegroundColor Yellow }
function Write-Step ([string]$Msg)                       { Write-Host "==> $Msg" -ForegroundColor Cyan }

# Test-Tool <name> <scriptblock:check> <string:install-cmd> [-HintOnly]
# <install-cmd> is both the hint (printed in verify mode) and the command
# executed via Invoke-Expression in -CI mode. -HintOnly skips auto-install
# even in -CI (use when the "hint" is natural-language or a dedicated CI
# action installs it).
function Test-Tool {
    param(
        [string]$Name,
        [scriptblock]$Check,
        [string]$InstallCmd,
        [switch]$HintOnly
    )

    $passed = $false
    try { $passed = [bool](& $Check) } catch { $passed = $false }

    if ($passed) {
        Write-Ok $Name
        return
    }

    if ($script:Mode -eq 'install' -and -not $HintOnly) {
        Write-Step "installing: $Name"
        try {
            Invoke-Expression $InstallCmd | Out-Null
            $passed = $false
            try { $passed = [bool](& $Check) } catch { $passed = $false }
            if ($passed) {
                Write-Ok "$Name (installed)"
                return
            }
            Write-Miss $Name 'install failed'
        } catch {
            Write-Miss $Name "install error: $_"
        }
    } else {
        Write-Miss $Name $InstallCmd
    }
    [void]$script:Missing.Add($Name)
}

function Get-PnpmPinnedVersion {
    $pkgPath = Join-Path $script:Root 'package.json'
    if (-not (Test-Path $pkgPath)) { return $null }
    $pkg = Get-Content -Raw -Path $pkgPath | ConvertFrom-Json
    if ($pkg.packageManager -match '^pnpm@([0-9.]+)') { return $Matches[1] }
    return $null
}

function Test-VswhereHasMsvc {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { return $false }
    $path = & $vswhere -latest -products * `
        -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 `
        -property installationPath 2>$null
    return [bool]$path
}

function Invoke-SharedChecks {
    Write-Step "mode: $script:Mode"
    Write-Step "os:   Windows ($([System.Environment]::OSVersion.Version))"
    Write-Step "root: $script:Root"

    Write-Step 'Windows dependencies'

    Test-Tool 'winget' { Get-Command winget -ErrorAction SilentlyContinue } `
        'install App Installer from the Microsoft Store' -HintOnly

    Test-Tool 'cmake' { Get-Command cmake -ErrorAction SilentlyContinue } `
        'winget install --accept-source-agreements --accept-package-agreements -e --id Kitware.CMake'

    Test-Tool 'MSVC Build Tools' { Test-VswhereHasMsvc } `
        'install Visual Studio Build Tools (workload: Desktop development with C++). CI uses TheMrMilchmann/setup-msvc-dev.' -HintOnly

    Test-Tool 'rustup' { Get-Command rustup -ErrorAction SilentlyContinue } `
        'winget install -e --id Rustlang.Rustup -- then: rustup default stable' -HintOnly

    Test-Tool 'Node.js' { Get-Command node -ErrorAction SilentlyContinue } `
        'winget install -e --id OpenJS.NodeJS.LTS' -HintOnly

    $pinned = Get-PnpmPinnedVersion
    $pnpmName = if ($pinned) { "pnpm $pinned" } else { 'pnpm' }
    $pnpmPkg  = if ($pinned) { "pnpm@$pinned" } else { 'pnpm@latest' }
    Test-Tool $pnpmName { Get-Command pnpm -ErrorAction SilentlyContinue } `
        "corepack enable; corepack prepare $pnpmPkg --activate"

    Write-Step 'release scripting tools'

    if (-not (Get-Command go -ErrorAction SilentlyContinue)) {
        Write-Warn 'vergo/sumry' 'install Go (https://go.dev/dl/) to enable release scripts'
    } else {
        $goBin = (& go env GOBIN).Trim()
        if (-not $goBin) { $goBin = Join-Path (& go env GOPATH).Trim() 'bin' }

        if (($env:PATH -split ';') -notcontains $goBin) {
            Write-Warn 'PATH' "$goBin not on PATH -- add it to use vergo/sumry"
            $env:PATH = "$goBin;$env:PATH"
        }

        foreach ($tool in 'vergo', 'sumry') {
            Test-Tool $tool { Get-Command $tool -ErrorAction SilentlyContinue } `
                "go install github.com/xjerod/sumry/cmd/$tool@latest"
        }
    }
}

function Initialize-Submodules {
    $gitmodules = Join-Path $script:Root '.gitmodules'
    if (Test-Path $gitmodules) {
        Write-Step 'git submodule update --init --recursive'
        git submodule update --init --recursive
    }
}

Invoke-SharedChecks

$projectExtras = Join-Path $script:Root '.install_deps.ps1'
if (Test-Path $projectExtras) {
    Write-Step "per-project extras: $projectExtras"
    . $projectExtras
}

Initialize-Submodules

Write-Host ''
if ($script:Missing.Count -eq 0) {
    $msg = 'all good.'
    if ($script:Mode -eq 'verify') { $msg += ' next: pnpm install; pnpm tauri dev' }
    Write-Host $msg -ForegroundColor Green
    exit 0
}

Write-Host ("{0} missing: {1}" -f $script:Missing.Count, ($script:Missing -join ', ')) -ForegroundColor Red
if ($script:Mode -eq 'verify') { Write-Host 're-run with -CI to install automatically' }
exit 1
