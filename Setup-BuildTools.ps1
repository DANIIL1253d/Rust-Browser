param([Parameter(Mandatory=$true)][string]$PythonPath)
$ErrorActionPreference = 'Stop'
$taskWork = & (Join-Path $PSScriptRoot 'Get-BrowserWork.ps1') -ProjectRoot $PSScriptRoot
$toolsDir = Join-Path $taskWork 'build-tools'
& $PythonPath -m pip install --target $toolsDir cmake==4.4.4 ninja==1.13.2 libclang==18.1.1
if ($LASTEXITCODE -ne 0) { throw 'Failed to install build dependencies.' }
