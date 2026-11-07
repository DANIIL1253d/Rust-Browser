$ErrorActionPreference = 'Stop'
$taskWork = & (Join-Path $PSScriptRoot 'Get-BrowserWork.ps1') -ProjectRoot $PSScriptRoot
$downloadDir = Join-Path $taskWork 'network-downloads'
New-Item -ItemType Directory -Path $downloadDir -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $PSScriptRoot 'runtime') -Force | Out-Null
$engines = @(
    @{ Name='sing-box'; Url='https://github.com/SagerNet/sing-box/releases/download/v1.14.4/sing-box-1.14.4-windows-amd64.zip'; Sha='99185d75d314a07dbba4a130c8ad986cbeb629a07927975636b341ca61f991df'; Binary='sing-box.exe' },
    @{ Name='xray'; Url='https://github.com/XTLS/Xray-core/releases/download/v26.10.31/Xray-windows-64.zip'; Sha='2e04a9995a6f2ad1b28c0bb25c708ecb2695e43a1c9c7889c4b6ac2b48cf7a6a'; Binary='xray.exe' }
)
foreach ($engine in $engines) {
    $archive = Join-Path $downloadDir ($engine.Name + '.zip')
    Invoke-WebRequest $engine.Url -OutFile $archive
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $engine.Sha) { throw "Archive digest mismatch: $($engine.Name)" }
    $extractDir = Join-Path $downloadDir $engine.Name
    Expand-Archive -LiteralPath $archive -DestinationPath $extractDir -Force
    $binary = Get-ChildItem -LiteralPath $extractDir -Recurse -Filter $engine.Binary | Select-Object -First 1
    if (-not $binary) { throw "Engine binary missing: $($engine.Name)" }
    Copy-Item -LiteralPath $binary.FullName -Destination (Join-Path $PSScriptRoot ('runtime\' + $engine.Binary)) -Force
}
