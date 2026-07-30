#Requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$ProjectRoot,

    [Parameter(Mandatory = $true)]
    [ValidateSet('build', 'build-internal')]
    [string]$BuildTarget,

    [AllowEmptyString()]
    [string]$Artifact = '',

    [string]$MakeCommand = 'make',

    [string]$CargoCommand = 'cargo'
)

$ErrorActionPreference = 'Stop'

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

function Resolve-Artifact {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $resolved = Resolve-Path -LiteralPath $Path -ErrorAction Stop
    $item = Get-Item -LiteralPath $resolved.Path
    if ($item.PSIsContainer) {
        throw "install artifact is not a file: $Path"
    }
    if ($item.Extension -ine '.exe') {
        throw "Windows installs require an .exe artifact: $($item.FullName)"
    }
    return $item.FullName
}

function Find-FreshInstaller {
    param(
        [Parameter(Mandatory = $true)]
        [string]$TargetDirectory,

        [Parameter(Mandatory = $true)]
        [datetime]$StartedAt
    )

    $bundleDir = Join-Path $TargetDirectory 'release\bundle\nsis'
    $matches = @()
    if (Test-Path -LiteralPath $bundleDir -PathType Container) {
        $matches = @(
            Get-ChildItem -LiteralPath $bundleDir -File -Filter '*-setup.exe' |
                Where-Object { $_.LastWriteTimeUtc -ge $StartedAt }
        )
    }

    if ($matches.Count -ne 1) {
        $details = if ($matches.Count -eq 0) {
            ''
        } else {
            "`n  " + (($matches.FullName) -join "`n  ")
        }
        throw "expected exactly one fresh NSIS installer, found $($matches.Count)$details"
    }
    return $matches[0].FullName
}

function Invoke-Installer {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $process = Start-Process -FilePath $Path -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "Tauri installer failed with exit code $($process.ExitCode)"
    }
}

$repoRoot = (Resolve-Path -LiteralPath $ProjectRoot -ErrorAction Stop).Path

if ([string]::IsNullOrWhiteSpace($Artifact)) {
    $metadataJson = & $CargoCommand 'metadata' `
        '--manifest-path' (Join-Path $repoRoot 'src-tauri\Cargo.toml') `
        '--format-version' '1' `
        '--no-deps'
    if ($LASTEXITCODE -ne 0) {
        throw "$CargoCommand metadata failed with exit code $LASTEXITCODE"
    }
    $metadata = ($metadataJson -join [Environment]::NewLine) | ConvertFrom-Json
    if ([string]::IsNullOrWhiteSpace([string]$metadata.target_directory)) {
        throw 'cargo metadata did not return target_directory'
    }

    $startedAt = [datetime]::UtcNow
    Invoke-Native $MakeCommand @('--no-print-directory', '-C', $repoRoot, $BuildTarget)
    $installer = Find-FreshInstaller `
        -TargetDirectory ([string]$metadata.target_directory) `
        -StartedAt $startedAt
} else {
    $installer = Resolve-Artifact -Path $Artifact
}

Invoke-Installer -Path $installer
