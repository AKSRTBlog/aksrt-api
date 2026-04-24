param(
    [string]$Image = "kate522/aksrtblog-api",
    [string]$Tag = "",
    [string]$Platform = "linux/amd64",
    [switch]$Push,
    [switch]$UseCache,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$BackendDir = Split-Path -Parent $ScriptDir

if ([string]::IsNullOrWhiteSpace($Tag)) {
    $timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
    $gitSha = ""
    try {
        $gitSha = (git -C $BackendDir rev-parse --short HEAD).Trim()
    } catch {
        $gitSha = "nogit"
    }
    $Tag = "$timestamp-$gitSha"
}

$buildArgs = @(
    "buildx", "build",
    "--platform", $Platform,
    "--pull",
    "-t", "${Image}:${Tag}",
    "-t", "${Image}:latest"
)

if (-not $UseCache) {
    $buildArgs += "--no-cache"
}

if ($Push) {
    $buildArgs += "--push"
} else {
    $buildArgs += "--load"
}

$buildArgs += $BackendDir

Write-Host "Building API image:" -ForegroundColor Cyan
Write-Host "  ${Image}:${Tag}"
Write-Host "  ${Image}:latest"
Write-Host ""
Write-Host "Command:" -ForegroundColor Cyan
Write-Host "docker $($buildArgs -join ' ')"

if ($DryRun) {
    exit 0
}

docker @buildArgs

