param([Parameter(Mandatory=$true)][string]$ProjectRoot)
if ($env:RUST_BROWSER_WORK) { [IO.Path]::GetFullPath($env:RUST_BROWSER_WORK); return }
$legacy = [IO.Path]::GetFullPath((Join-Path $ProjectRoot '..\..\work'))
if (Test-Path -LiteralPath (Join-Path $legacy 'build-tools')) { $legacy; return }
Join-Path $ProjectRoot '.build'
