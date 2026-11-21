param([Parameter(Mandatory=$true)][string]$PythonPath)
$ErrorActionPreference = 'Stop'
$taskWork = & (Join-Path $PSScriptRoot 'Get-BrowserWork.ps1') -ProjectRoot $PSScriptRoot
$toolsDir = Join-Path $taskWork 'build-tools'
& $PythonPath -m pip install --target $toolsDir cmake==4.4.4 ninja==1.13.2
if ($LASTEXITCODE -ne 0) { throw 'Failed to install build dependencies.' }
$llvmVersion = '23.1.3'
$llvmDirectory = Join-Path $toolsDir "clang+llvm-$llvmVersion-x86_64-pc-windows-msvc"
if (-not (Test-Path -LiteralPath (Join-Path $llvmDirectory 'bin/clang-cl.exe'))) {
    $archive = Join-Path $toolsDir "clang+llvm-$llvmVersion-x86_64-pc-windows-msvc.tar.xz"
    if (-not (Test-Path -LiteralPath $archive)) {
        Invoke-WebRequest -Uri "https://github.com/llvm/llvm-project/releases/download/llvmorg-$llvmVersion/clang%2Bllvm-$llvmVersion-x86_64-pc-windows-msvc.tar.xz" -OutFile $archive
    }
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne '9ee6a2192fbb30c776415af529927f76ca6b5d1d55699bb1315f9878e610b5b8') { throw 'LLVM archive differs from the official release SHA256.' }
    & tar.exe -xf $archive -C $toolsDir
    if ($LASTEXITCODE -ne 0) { throw 'Failed to extract the LLVM toolchain.' }
}
