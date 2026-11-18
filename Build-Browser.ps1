param([switch]$Release, [switch]$CoreTests, [switch]$NetworkTests, [switch]$UiTests, [string]$FixtureOutput, [string]$PythonPath, [string]$GStreamerRoot)
$ErrorActionPreference = 'Stop'
$projectRoot = $PSScriptRoot
$taskWork = & (Join-Path $PSScriptRoot 'Get-BrowserWork.ps1') -ProjectRoot $projectRoot
$sourceRoot = & (Join-Path $PSScriptRoot 'Initialize-ServoSource.ps1')
$toolsDir = Join-Path $taskWork 'build-tools'
$vsWhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
$vsInstall = & $vsWhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vsInstall) { throw 'Install the Visual Studio C++ build tools and Windows SDK.' }
$devCmd = Join-Path $vsInstall 'Common7\Tools\VsDevCmd.bat'
$envLines = & cmd.exe /d /s /c "`"$devCmd`" -no_logo -arch=amd64 && set"
foreach ($line in $envLines) {
    if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process') }
}
$env:PATH = "$toolsDir\clang+llvm-23.1.3-x86_64-pc-windows-msvc\bin;$toolsDir\cmake\data\bin;$toolsDir\ninja\data\bin;$toolsDir\bin;$env:PATH"
if (-not (Get-Command clang-cl.exe -ErrorAction SilentlyContinue)) { throw 'LLVM is required by Servo 0.8. Run Setup-BuildTools.ps1 first.' }
$env:LIBCLANG_PATH = Join-Path $toolsDir 'clang+llvm-23.1.3-x86_64-pc-windows-msvc\bin'
if (-not $PythonPath) { $PythonPath = $env:PYTHON3 }
if (-not $PythonPath) { $PythonPath = (Get-Command python.exe -ErrorAction SilentlyContinue).Source }
if (-not $PythonPath -or -not (Test-Path -LiteralPath $PythonPath)) { throw 'Python 3.11+ is required. Pass -PythonPath with its executable path.' }
$env:PYTHON3 = $PythonPath
$env:PYTHON = $env:PYTHON3
$env:PATH = "$(Split-Path $env:PYTHON3);$env:PATH"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = 'link.exe'
if (-not $env:CARGO_TARGET_DIR) {
    $targetName = if ($CoreTests -or $FixtureOutput -or $UiTests) { 'browser-core-target' } else { 'browser-target' }
    $env:CARGO_TARGET_DIR = Join-Path $taskWork $targetName
}
$env:CARGO_BUILD_JOBS = '2'
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:CARGO_PROFILE_DEV_INCREMENTAL = 'false'
$env:CARGO_PROFILE_DEV_CODEGEN_UNITS = '16'
$env:CARGO_PROFILE_TEST_DEBUG = '0'
$env:CARGO_PROFILE_TEST_INCREMENTAL = 'false'
$normalBuild = -not ($CoreTests -or $UiTests -or $NetworkTests -or $FixtureOutput)
if ($normalBuild) {
    if (-not $GStreamerRoot) { $GStreamerRoot = $env:GSTREAMER_1_0_ROOT_MSVC_X86_64 }
    if (-not $GStreamerRoot) { $GStreamerRoot = Join-Path $taskWork 'gstreamer-1.28.8' }
    $GStreamerRoot = [IO.Path]::GetFullPath($GStreamerRoot)
    if (-not (Test-Path -LiteralPath (Join-Path $GStreamerRoot 'lib\pkgconfig\gstreamer-mse-1.0.pc'))) { throw 'Install the media SDK with Setup-MediaTools.ps1, or pass -GStreamerRoot.' }
    $env:PATH = "$GStreamerRoot\bin;$env:PATH"
    $env:PKG_CONFIG = Join-Path $GStreamerRoot 'bin\pkg-config.exe'
    $env:PKG_CONFIG_PATH = Join-Path $GStreamerRoot 'lib\pkgconfig'
    $env:GSTREAMER_1_0_ROOT_MSVC_X86_64 = $GStreamerRoot
    $env:LIB = "$GStreamerRoot\lib;$env:LIB"
    $crtDirectory = Join-Path $env:VCToolsRedistDir 'x64\Microsoft.VC143.CRT'
    if (-not (Test-Path -LiteralPath (Join-Path $crtDirectory 'vcruntime140.dll'))) { throw 'The Visual Studio x64 CRT redistributable folder is missing.' }
}
Push-Location $sourceRoot
try {
    $features = 'bundled,clipboard,js_jit,multiprocess,webgl,subtlecrypto,media-gstreamer'
    $cargoArgs = @('+stable', 'build', '--locked', '-p', 'servoshell', '--no-default-features', '--features', $features)
    if ($CoreTests) { $cargoArgs = @('+stable', 'test', '--locked', '--manifest-path', 'ports/servoshell/browser-core/Cargo.toml') }
    if ($UiTests) { $cargoArgs = @('+stable', 'test', '--locked', '--manifest-path', 'tests/browser-ui/Cargo.toml') }
    if ($NetworkTests) { $cargoArgs = @('+stable', 'test', '--locked', '-p', 'servo-net', '--lib', 'browser_routing_', '--', '--test-threads=1') }
    if ($FixtureOutput) { $cargoArgs = @('+stable', 'run', '--locked', '-p', 'rust-browser-core', '--example', 'connection_fixtures', '--', $FixtureOutput) }
    if ($Release) { $cargoArgs = @($cargoArgs[0], $cargoArgs[1], '--release') + $cargoArgs[2..($cargoArgs.Length - 1)] }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "Cargo build failed ($LASTEXITCODE)." }
    if ($normalBuild) {
        $configuration = if ($Release) { 'release' } else { 'debug' }
        $binary = Join-Path $env:CARGO_TARGET_DIR "$configuration\servoshell.exe"
        [ordered]@{ profile = $configuration; features = $features; media_sdk = $GStreamerRoot; crt = $crtDirectory; binary_sha256 = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant(); python = $PythonPath } | ConvertTo-Json | Set-Content -Encoding UTF8 (Join-Path $env:CARGO_TARGET_DIR "$configuration\rust-browser-build.json")
    }
} finally { Pop-Location }
