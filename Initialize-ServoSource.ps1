$ErrorActionPreference = 'Stop'
$projectRoot = $PSScriptRoot
if (Test-Path -LiteralPath (Join-Path $projectRoot 'components\script\lib.rs')) { Write-Output $projectRoot; return }
$taskWork = & (Join-Path $projectRoot 'Get-BrowserWork.ps1') -ProjectRoot $projectRoot
$sourceRoot = Join-Path $taskWork 'servo-source'
$baseline = 'aac43a3f31a259f04a574f5ec4e959c943ad7cc7'
New-Item -ItemType Directory -Path $taskWork -Force | Out-Null
if (-not (Test-Path -LiteralPath (Join-Path $sourceRoot '.git'))) {
    & git -c core.longpaths=true clone --filter=blob:none --no-checkout --depth=1 --branch v0.7.0 https://github.com/servo/servo.git $sourceRoot | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Cannot download the pinned Servo source.' }
    & git -C $sourceRoot sparse-checkout init --cone | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Cannot initialize the Servo workspace.' }
    & git -C $sourceRoot sparse-checkout set .cargo components etc ffi ports python resources support tests/unit tests/capi | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Cannot select the Servo build sources.' }
    & git -C $sourceRoot checkout --detach $baseline | Out-Host
    if ($LASTEXITCODE -ne 0) { throw 'Cannot check out the pinned Servo revision.' }
}
$revision = & git -C $sourceRoot rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $revision -ne $baseline) { throw 'Unexpected Servo revision; existing workspace was preserved.' }
$overlay = Get-Content -LiteralPath (Join-Path $projectRoot 'BROWSER-OVERLAY.json') -Raw | ConvertFrom-Json
foreach ($relative in $overlay.files) {
    if ([IO.Path]::IsPathRooted($relative) -or $relative -match '(^|/)\.\.(/|$)' -or $relative -match '^(app|\.build)/') { throw 'Invalid source overlay path.' }
    $source = Join-Path $projectRoot $relative
    $target = Join-Path $sourceRoot $relative
    New-Item -ItemType Directory -Path (Split-Path $target) -Force | Out-Null
    Copy-Item -LiteralPath $source -Destination $target -Force
}
Write-Output $sourceRoot
