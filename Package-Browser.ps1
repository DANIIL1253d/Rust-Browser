param([string]$AppDirectory, [switch]$Release)
$ErrorActionPreference = 'Stop'
$projectRoot = $PSScriptRoot
$configuration = if ($Release) { 'release' } else { 'debug' }
$taskWork = & (Join-Path $PSScriptRoot 'Get-BrowserWork.ps1') -ProjectRoot $projectRoot
$sourceRoot = if (Test-Path -LiteralPath (Join-Path $projectRoot 'components\script\lib.rs')) { $projectRoot } else { Join-Path $taskWork 'servo-source' }
if (-not (Test-Path -LiteralPath (Join-Path $sourceRoot 'resources'))) { throw 'Build the Servo workspace before packaging.' }
$targetRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $taskWork 'browser-target' }
$targetDir = Join-Path $targetRoot $configuration
$appDir = if ($AppDirectory) { [IO.Path]::GetFullPath($AppDirectory) } else { Join-Path $projectRoot 'app' }
$binary = Join-Path $targetDir 'servoshell.exe'
if (-not (Test-Path -LiteralPath $binary)) { throw 'Build the browser first using Build-Browser.ps1.' }
$build = Get-Content -LiteralPath (Join-Path $targetDir 'rust-browser-build.json') -Raw | ConvertFrom-Json
if ($build.profile -ne $configuration -or $build.features.Split(',') -notcontains 'media-gstreamer' -or $build.binary_sha256 -ne (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant()) { throw 'The executable does not match a recorded media-enabled build. Rebuild before packaging.' }
New-Item -ItemType Directory -Path $appDir -Force | Out-Null
Copy-Item -LiteralPath $binary -Destination (Join-Path $appDir 'RustBrowser.exe')
Copy-Item -LiteralPath (Join-Path $sourceRoot 'resources') -Destination $appDir -Recurse -Force
Copy-Item -LiteralPath (Join-Path $projectRoot 'runtime') -Destination $appDir -Recurse -Force
Get-ChildItem -LiteralPath $targetDir -Filter '*.dll' | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $appDir -Force }
Get-ChildItem -LiteralPath (Join-Path $targetDir 'build') -Recurse -Filter '*.dll' | Where-Object Name -In @('libEGL.dll','libGLESv2.dll','mozjs.dll') | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination $appDir -Force }
& $build.python (Join-Path $projectRoot 'tools\bundle_media.py') --source $sourceRoot --sdk $build.media_sdk --crt $build.crt --output $appDir --executable (Join-Path $appDir 'RustBrowser.exe')
if ($LASTEXITCODE -ne 0) { throw 'Media dependency packaging failed.' }
Copy-Item -LiteralPath (Join-Path $projectRoot 'LICENSE') -Destination (Join-Path $appDir 'LICENSE-Servo.txt')
Copy-Item -LiteralPath (Join-Path $projectRoot 'licenses\Microsoft-Visual-C++-Runtime.txt') -Destination (Join-Path $appDir 'licenses\Microsoft-Visual-C++-Runtime.txt')
Write-Output (Join-Path $appDir 'RustBrowser.exe')
