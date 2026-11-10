param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$packageManifest = Get-Content -Raw (Join-Path $repositoryRoot "package.json") | ConvertFrom-Json
$version = $packageManifest.version
$buildRoot = Join-Path $repositoryRoot "artifacts\build-$version"
$releaseRoot = Join-Path $repositoryRoot "artifacts\releases\$version"

if (-not $SkipBuild) {
    $env:CARGO_TARGET_DIR = $buildRoot
    Push-Location $repositoryRoot
    try {
        & npm.cmd run tauri -- build
        if ($LASTEXITCODE -ne 0) {
            throw "Tauri build failed with exit code $LASTEXITCODE."
        }
    }
    finally {
        Pop-Location
    }
}

$releaseFiles = @(
    @{
        Source = Join-Path $buildRoot "release\bundle\msi\DirMover_${version}_x64_zh-CN.msi"
        Name = "DirMover_${version}_x64_zh-CN.msi"
    }
)

foreach ($file in $releaseFiles) {
    if (-not (Test-Path -LiteralPath $file.Source -PathType Leaf)) {
        throw "Release artifact not found: $($file.Source)"
    }
}

New-Item -ItemType Directory -Path $releaseRoot -Force | Out-Null
foreach ($file in $releaseFiles) {
    Copy-Item -LiteralPath $file.Source -Destination (Join-Path $releaseRoot $file.Name) -Force
}

Write-Host "Release artifacts are ready in: $releaseRoot"
Get-ChildItem -LiteralPath $releaseRoot -File | Select-Object Name, Length
