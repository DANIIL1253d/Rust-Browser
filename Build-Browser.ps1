param([switch]$Release, [switch]$CoreTests, [switch]$NetworkTests, [switch]$UiTests, [string]$FixtureOutput, [string]$PythonPath)
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
$env:PATH = "$toolsDir\cmake\data\bin;$toolsDir\ninja\data\bin;$toolsDir\bin;$env:PATH"
$env:LIBCLANG_PATH = Join-Path $toolsDir 'clang\native'
# libclang is used to parse binding declarations; native C++ compilation uses
# the installed MSVC toolset. Permit this parser to read newer MSVC STL headers.
$env:BINDGEN_EXTRA_CLANG_ARGS = '-D_ALLOW_COMPILER_AND_STL_VERSION_MISMATCH'
if (-not $PythonPath) { $PythonPath = $env:PYTHON3 }
if (-not $PythonPath) { $PythonPath = (Get-Command python.exe -ErrorAction SilentlyContinue).Source }
if (-not $PythonPath -or -not (Test-Path -LiteralPath $PythonPath)) { throw 'Python 3.11+ is required. Pass -PythonPath with its executable path.' }
$env:PYTHON3 = $PythonPath
$env:PYTHON = $env:PYTHON3
$env:PATH = "$(Split-Path $env:PYTHON3);$env:PATH"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = 'link.exe'
$env:CARGO_TARGET_DIR = Join-Path $taskWork 'browser-target'
if ($CoreTests -or $FixtureOutput -or $UiTests) { $env:CARGO_TARGET_DIR = Join-Path $taskWork 'browser-core-target' }
$env:CARGO_BUILD_JOBS = '2'
$env:CARGO_PROFILE_DEV_DEBUG = '0'
$env:CARGO_PROFILE_DEV_INCREMENTAL = 'false'
$env:CARGO_PROFILE_DEV_CODEGEN_UNITS = '16'
$env:CARGO_PROFILE_TEST_DEBUG = '0'
$env:CARGO_PROFILE_TEST_INCREMENTAL = 'false'
Push-Location $sourceRoot
try {
    $cargoArgs = @('+stable', 'build', '-p', 'servoshell', '--no-default-features', '--features', 'bundled,clipboard,js_jit,multiprocess,webgl,webcrypto')
    if ($CoreTests) { $cargoArgs = @('+stable', 'test', '--manifest-path', 'ports/servoshell/browser-core/Cargo.toml') }
    if ($UiTests) { $cargoArgs = @('+stable', 'test', '--manifest-path', 'tests/browser-ui/Cargo.toml') }
    if ($NetworkTests) { $cargoArgs = @('+stable', 'test', '-p', 'servo-net', '--lib', 'browser_routing_', '--', '--test-threads=1') }
    if ($FixtureOutput) { $cargoArgs = @('+stable', 'run', '-p', 'rust-browser-core', '--example', 'connection_fixtures', '--', $FixtureOutput) }
    if ($Release) { $cargoArgs += '--release' }
    & cargo @cargoArgs
    if ($LASTEXITCODE -ne 0) { throw "Cargo build failed ($LASTEXITCODE)." }
} finally { Pop-Location }
