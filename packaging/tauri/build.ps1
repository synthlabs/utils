#Requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$ProjectRoot,

    [string]$TokeCommand = 'toke',

    [string]$PnpmCommand = 'pnpm'
)

$ErrorActionPreference = 'Stop'
$projectPath = (Resolve-Path -LiteralPath $ProjectRoot -ErrorAction Stop).Path
Set-Location -LiteralPath $projectPath

& $TokeCommand '-v' $PnpmCommand 'tauri' 'build' '--bundles' 'nsis'
if ($LASTEXITCODE -ne 0) {
    throw "$TokeCommand failed with exit code $LASTEXITCODE"
}
