param([string]$Destination)
$ErrorActionPreference = 'Stop'
$taskWork = & (Join-Path $PSScriptRoot 'Get-BrowserWork.ps1') -ProjectRoot $PSScriptRoot
$version = '1.28.8'
$expectedHash = 'e4c41ee6ee9c6c32a45d198463ec08d79bac3bd5fa58adc93d13fe4a08abc9d9'
$root = if ($Destination) { [IO.Path]::GetFullPath($Destination) } else { Join-Path $taskWork "gstreamer-$version" }
$probe = Join-Path $root 'lib\pkgconfig\gstreamer-mse-1.0.pc'
if (Test-Path -LiteralPath $probe) { Write-Output $root; return }
$downloads = Join-Path $taskWork 'gstreamer-downloads'
New-Item -ItemType Directory -Path $downloads -Force | Out-Null
$installer = Join-Path $downloads "gstreamer-1.0-msvc-x86_64-$version.exe"
if (-not (Test-Path -LiteralPath $installer)) {
    Invoke-WebRequest -Uri "https://gstreamer.freedesktop.org/data/pkg/windows/$version/msvc/gstreamer-1.0-msvc-x86_64-$version.exe" -OutFile $installer
}
if ((Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant() -ne $expectedHash) { throw 'GStreamer download is incomplete or its SHA256 differs from the pinned official checksum. Remove the incomplete download and retry.' }
# Official SDK setup is per-user, without elevation, in a developer work folder.
# This is a build prerequisite; the browser package contains its own runtime DLLs.
$arguments = @('/CURRENTUSER','/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART','/NOICONS','/TYPE=devel',('/DIR="' + $root + '"'))
$process = Start-Process -FilePath $installer -ArgumentList $arguments -WindowStyle Hidden -Wait -PassThru
if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $probe)) { throw "GStreamer SDK setup failed: $($process.ExitCode)." }
Write-Output $root
