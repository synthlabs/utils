#Requires -Version 5.1
$ErrorActionPreference = 'Stop'

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$installScript = Join-Path $scriptDir 'install.ps1'
$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) "tauri-packaging-install-$([guid]::NewGuid())"
$projectRoot = Join-Path $testRoot 'repo with spaces'
$targetRoot = Join-Path $projectRoot 'nested target'
$makeLog = Join-Path $testRoot 'make.log'
$fakeMake = Join-Path $testRoot 'fake-make.cmd'
$fakeArtifact = Join-Path $testRoot 'fake-artifact.ps1'
$fakeCargo = Join-Path $testRoot 'fake-cargo.cmd'
$hostExe = (Get-Process -Id $PID).Path

function Assert-True {
    param(
        [bool]$Condition,
        [string]$Message
    )
    if (-not $Condition) {
        throw "FAIL: $Message"
    }
}

function Invoke-InstallTest {
    param(
        [string]$BuildTarget = 'build',
        [string]$Artifact
    )

    $arguments = @(
        '-NoProfile',
        '-ExecutionPolicy', 'Bypass',
        '-File', $installScript,
        '-ProjectRoot', $projectRoot,
        '-BuildTarget', $BuildTarget,
        '-MakeCommand', $fakeMake,
        '-CargoCommand', $fakeCargo
    )
    if ($PSBoundParameters.ContainsKey('Artifact')) {
        $arguments += @('-Artifact', $Artifact)
    }

    $previousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $output = & $hostExe @arguments 2>&1
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }
    return @{
        ExitCode = $exitCode
        Output = ($output -join "`n")
    }
}

function Reset-TestState {
    Remove-Item -LiteralPath $targetRoot -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $makeLog -Force -ErrorAction SilentlyContinue
    $env:TAURI_PACKAGING_TEST_MAKE_MODE = 'one'
    $env:TAURI_PACKAGING_TEST_MAKE_EXIT_CODE = '0'
}

New-Item -ItemType Directory -Path (Join-Path $projectRoot 'src-tauri') -Force | Out-Null
New-Item -ItemType File -Path (Join-Path $projectRoot 'src-tauri\Cargo.toml') -Force | Out-Null

@'
@echo off
echo {"target_directory":"%TAURI_PACKAGING_TEST_TARGET_DIR_JSON%"}
'@ | Set-Content -LiteralPath $fakeCargo -Encoding Ascii

@'
param(
    [Parameter(Mandatory = $true)]
    [string]$Destination
)
$ErrorActionPreference = 'Stop'
Copy-Item -LiteralPath (Join-Path $env:SystemRoot 'System32\whoami.exe') -Destination $Destination
(Get-Item -LiteralPath $Destination).LastWriteTimeUtc = [datetime]::UtcNow.AddSeconds(5)
'@ | Set-Content -LiteralPath $fakeArtifact -Encoding UTF8

@'
@echo off
echo %*>>"%TAURI_PACKAGING_TEST_MAKE_LOG%"
if "%TAURI_PACKAGING_TEST_MAKE_MODE%"=="fail" exit /b %TAURI_PACKAGING_TEST_MAKE_EXIT_CODE%
if "%TAURI_PACKAGING_TEST_MAKE_MODE%"=="missing" exit /b 0
if not exist "%TAURI_PACKAGING_TEST_TARGET_DIR%\release\bundle\nsis" mkdir "%TAURI_PACKAGING_TEST_TARGET_DIR%\release\bundle\nsis"
if "%TAURI_PACKAGING_TEST_MAKE_MODE%"=="multiple" (
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%TAURI_PACKAGING_TEST_ARTIFACT_SCRIPT%" "%TAURI_PACKAGING_TEST_TARGET_DIR%\release\bundle\nsis\Sample_one_x64-setup.exe" || exit /b 1
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%TAURI_PACKAGING_TEST_ARTIFACT_SCRIPT%" "%TAURI_PACKAGING_TEST_TARGET_DIR%\release\bundle\nsis\Sample_two_x64-setup.exe" || exit /b 1
  exit /b 0
)
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%TAURI_PACKAGING_TEST_ARTIFACT_SCRIPT%" "%TAURI_PACKAGING_TEST_TARGET_DIR%\release\bundle\nsis\Sample_1.0.0_x64-setup.exe" || exit /b 1
exit /b 0
'@ | Set-Content -LiteralPath $fakeMake -Encoding Ascii

$env:TAURI_PACKAGING_TEST_TARGET_DIR = $targetRoot
$env:TAURI_PACKAGING_TEST_TARGET_DIR_JSON = $targetRoot.Replace('\', '/')
$env:TAURI_PACKAGING_TEST_MAKE_LOG = $makeLog
$env:TAURI_PACKAGING_TEST_ARTIFACT_SCRIPT = $fakeArtifact

try {
    Reset-TestState
    $result = Invoke-InstallTest
    Assert-True ($result.ExitCode -eq 0) "normal install failed: $($result.Output)"
    $makeArgs = Get-Content -Raw -LiteralPath $makeLog
    Assert-True ($makeArgs -match 'build\s*$') 'normal install did not run the build target'

    Reset-TestState
    $result = Invoke-InstallTest -BuildTarget 'build-internal'
    Assert-True ($result.ExitCode -eq 0) "internal install failed: $($result.Output)"
    $makeArgs = Get-Content -Raw -LiteralPath $makeLog
    Assert-True ($makeArgs -match 'build-internal\s*$') 'internal install did not run build-internal'

    Reset-TestState
    $overrideDir = Join-Path $testRoot 'override packages'
    New-Item -ItemType Directory -Path $overrideDir -Force | Out-Null
    $override = Join-Path $overrideDir 'Sample local.exe'
    Copy-Item -LiteralPath (Join-Path $env:SystemRoot 'System32\whoami.exe') -Destination $override
    $result = Invoke-InstallTest -Artifact $override
    Assert-True ($result.ExitCode -eq 0) "override install failed: $($result.Output)"
    Assert-True (-not (Test-Path -LiteralPath $makeLog)) 'artifact override unexpectedly ran the build'

    Reset-TestState
    $badOverride = Join-Path $overrideDir 'Sample.deb'
    New-Item -ItemType File -Path $badOverride -Force | Out-Null
    $result = Invoke-InstallTest -Artifact $badOverride
    Assert-True ($result.ExitCode -ne 0) 'Windows install accepted a non-EXE override'

    Reset-TestState
    $env:TAURI_PACKAGING_TEST_MAKE_MODE = 'missing'
    $result = Invoke-InstallTest
    Assert-True ($result.ExitCode -ne 0) 'Windows install accepted a missing fresh artifact'

    Reset-TestState
    $env:TAURI_PACKAGING_TEST_MAKE_MODE = 'multiple'
    $result = Invoke-InstallTest
    Assert-True ($result.ExitCode -ne 0) 'Windows install accepted multiple fresh artifacts'

    Reset-TestState
    $env:TAURI_PACKAGING_TEST_MAKE_MODE = 'fail'
    $env:TAURI_PACKAGING_TEST_MAKE_EXIT_CODE = '31'
    $result = Invoke-InstallTest
    Assert-True ($result.ExitCode -ne 0) 'Windows install ignored a build failure'

    Reset-TestState
    $failingInstaller = Join-Path $overrideDir 'Sample failing.exe'
    Copy-Item -LiteralPath (Join-Path $env:SystemRoot 'System32\where.exe') -Destination $failingInstaller
    $result = Invoke-InstallTest -Artifact $failingInstaller
    Assert-True ($result.ExitCode -ne 0) 'Windows install ignored the installer exit code'

    Write-Host 'install PowerShell tests passed'
} finally {
    Remove-Item Env:TAURI_PACKAGING_TEST_TARGET_DIR -ErrorAction SilentlyContinue
    Remove-Item Env:TAURI_PACKAGING_TEST_TARGET_DIR_JSON -ErrorAction SilentlyContinue
    Remove-Item Env:TAURI_PACKAGING_TEST_MAKE_LOG -ErrorAction SilentlyContinue
    Remove-Item Env:TAURI_PACKAGING_TEST_ARTIFACT_SCRIPT -ErrorAction SilentlyContinue
    Remove-Item Env:TAURI_PACKAGING_TEST_MAKE_MODE -ErrorAction SilentlyContinue
    Remove-Item Env:TAURI_PACKAGING_TEST_MAKE_EXIT_CODE -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $testRoot -Recurse -Force -ErrorAction SilentlyContinue
}
