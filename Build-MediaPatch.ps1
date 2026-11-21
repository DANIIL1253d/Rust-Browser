<#
.SYNOPSIS
Build the pinned GStreamer MSE initialization fix without modifying the SDK/app.
.DESCRIPTION
Requires an x64 MSVC GStreamer 1.28.8 SDK, Visual Studio C++ tools, Git and
Python 3. Sources/license are fetched from an immutable official upstream commit
and checked by SHA256. Output includes the DLL, ABI evidence and corresponding
source/build instructions. SourceBuffer removal/abort are not implemented here.
#>
param(
    [Parameter(Mandatory=$true)][string]$SdkRoot,
    [string]$Python = 'python',
    [string]$WorkDirectory,
    [string]$OutputDirectory
)
$ErrorActionPreference = 'Stop'
$commit = '1b2a95a1b72787cda146b46df9605b168e5b683b'
$patchName = '0001-mse-initial-stream-readiness.patch'
$patchHash = '0a543d70834594cb5ddbce0e3622667f548ea8a3a6821f2086c72a1392e1f070'
$patchedSourceHash = '69f9a26c1d3df48a4505f4f02e5ce78fd59662b807ab69e1bd89d08db5838d1c'
$sdk = [IO.Path]::GetFullPath($SdkRoot).TrimEnd('\','/')
$work = if ($WorkDirectory) { [IO.Path]::GetFullPath($WorkDirectory) } elseif (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'Get-BrowserWork.ps1')) {
    & (Join-Path $PSScriptRoot 'Get-BrowserWork.ps1') -ProjectRoot $PSScriptRoot
} else { Join-Path $PSScriptRoot 'target/media-patch' }
$output = if ($OutputDirectory) { [IO.Path]::GetFullPath($OutputDirectory) } else { Join-Path $work 'mse-patched' }
foreach ($protected in @($sdk,(Join-Path $PSScriptRoot 'app'))) {
    if ($output.Equals($protected,[StringComparison]::OrdinalIgnoreCase) -or $output.StartsWith($protected.TrimEnd('\','/') + '\',[StringComparison]::OrdinalIgnoreCase)) {
        throw 'The output must be separate from the SDK and application.'
    }
}
$pc = Join-Path $sdk 'lib/pkgconfig/gstreamer-mse-1.0.pc'
if (-not (Test-Path -LiteralPath $pc) -or -not ((Get-Content -LiteralPath $pc) -match '^Version:\s+1\.28\.8(?:\.\d+)?$')) {
    throw 'Pass the x64 MSVC GStreamer 1.28.8 development SDK with -SdkRoot.'
}
$pythonCommand = (Get-Command $Python -ErrorAction Stop).Source
$gitCommand = (Get-Command git -ErrorAction Stop).Source
$mkenums = Join-Path $sdk 'bin/glib-mkenums'
if (-not (Test-Path -LiteralPath $mkenums)) { throw 'The SDK lacks glib-mkenums.' }
$patch = Join-Path $PSScriptRoot ('vendor/gstreamer/' + $patchName)
if ((Get-FileHash -LiteralPath $patch -Algorithm SHA256).Hash.ToLowerInvariant() -ne $patchHash) { throw 'Unexpected MSE patch bytes.' }
$build = Join-Path $work ('mse-patch-build-' + [guid]::NewGuid().ToString('N'))
$sourceRoot = Join-Path $build 'source'
$sourceDirectory = Join-Path $sourceRoot 'gst/mse'
New-Item -ItemType Directory -Path $sourceDirectory -Force | Out-Null
New-Item -ItemType Directory -Path $output -Force | Out-Null

# Each path is relative to gst-plugins-bad in the pinned upstream commit.
$pinnedFiles = @'
gst/mse/gstappendpipeline-private.h d104c9fd21f19b9c26ed2e0d9efc0901cc36de659591d89d4b6f29ba23568dbd
gst/mse/gstappendpipeline.c 9ea030449a1c0a369a4b668b0a5d493822ea6d4a7c48d869856baf3a73d96e45
gst/mse/gstmediasource-private.h d42d37b4577db1de1dfd05c811d7b98746c91ff4a790482111870c00daf5d707
gst/mse/gstmediasource.c 917e34f0eec79c0ddb8aa3b01e227aa429492293238d1b25de2c83af4189341e
gst/mse/gstmediasource.h bf93e49701239cc015a5aa95d58cc1dc4c5334c6d157d77c1536b25f50b19519
gst/mse/gstmediasourcesamplemap-private.h d6c9af654e21a7f08d31896536f83835e8d204426326c228fda1255879e4bf9f
gst/mse/gstmediasourcesamplemap.c 82825c863cab51aa07a01eb021867201ca0c3ab058f4b9911e59ae254026795f
gst/mse/gstmediasourcetrack-private.h ed22fca8f0b10810d3066b0d3e267a7c62c07293ee9a3d823bb20fdfb180e7e4
gst/mse/gstmediasourcetrack.c 7d3081575cbf878e8e897f12182bbcc64fac64b34af872ecfdb0001d879fba4a
gst/mse/gstmediasourcetrackbuffer-private.h 9c4eb56fdce12f618aca4110a9019766e09e98697fd2a34b344e1d761b4b1eea
gst/mse/gstmediasourcetrackbuffer.c b7bf8229a11c13a8d50bcd8eca760455c54b449b3d5b4ff7985b99b99fe575b3
gst/mse/gstmseeventqueue-private.h a250b42037315904aeef2c31a2f2718ddf4911559247690218a87f9b58aed089
gst/mse/gstmseeventqueue.c 1639f1810382db70b83287a4b70d41727fbb78f6414940e96b7d333c8707b10c
gst/mse/gstmselogging-private.h 5a78e1afc003914475e31ec5510e03a57a7856f5ce388dda562a33d6a7690a82
gst/mse/gstmselogging.c 9936666a125e77c73497efbcc22e4c33fafd284561a213f8e55f04eec697c768
gst/mse/gstmsemediatype-private.h 654395b09137a57fcb676b53dabfebd3ec8ba46ad287c860818d6bb0a2ebf7d6
gst/mse/gstmsemediatype.c a00cc8f1b8c45c857fdba4aac38175b572c97e69b749d7cc9013740bb245168c
gst/mse/gstmsesrc-private.h 68267980cf14c394c58b1fc817ca538c576e8c34be24a3153f8b84dd36c38ab0
gst/mse/gstmsesrc.c cca9008a11696bc4fd19d2d0201c0acb2e4b017f880add0efa3133672d5d2e52
gst/mse/gstmsesrc.h c28310d283c897201524800ea879cfd727219649fa98ed4d75943f6c937851df
gst/mse/gstsourcebuffer-private.h 84d287a8a543771c97c4c18a5a666df9c0fcdb7bf554c2a62a1f17565dcaeb30
gst/mse/gstsourcebuffer.c 13492aa3f6666b1cc55bbf915475a42c678207539d315b410912bcd0ad2aac2c
gst/mse/gstsourcebuffer.h 8b1b98dee35c6d45b2569b742bf71208d2e6ccac11c6f87fbf4a50ce0e41fb3e
gst/mse/gstsourcebufferlist-private.h 8166c44d5887ca709d8d50775d6c8ee0ebf3796174ae310c8e8d1b9e4ad406da
gst/mse/gstsourcebufferlist.c 784a412ab668ec1e64ef152a29060e895245e0f94dc01d5d1d2e9d5604793cac
gst/mse/gstsourcebufferlist.h 2a55bb18d69efbee8c59d46852d1c4fef3af8646485ce188eb517fde9f0adb8c
gst/mse/meson.build 8de5a50cfa2a5c2ea554161ecb60f6e270c8cdaae45b53f5d38a3889e7ffc50c
gst/mse/mse-prelude.h 0a25e14319ca201af8298d794a47b0be5f13fa5746182fd742ab2fc111365a02
gst/mse/mse.h da46aea211dd3035e4005b2409c2845f4597f9514c7161bf16803533deea8b82
COPYING dc626520dcd53a22f727af3ee42c770e56c97a64fe3adb063799d8ab032fe551
'@
$manifest = @()
foreach ($line in ($pinnedFiles -split '\r?\n')) {
    if (-not $line.Trim()) { continue }
    $relative, $expectedHash = $line.Trim() -split '\s+',2
    $remote = if ($relative -eq 'COPYING') { 'COPYING' } else { 'gst-libs/' + $relative }
    $url = "https://raw.githubusercontent.com/GStreamer/gstreamer/$commit/subprojects/gst-plugins-bad/$remote"
    $destination = Join-Path $sourceRoot $relative
    Invoke-WebRequest -Uri $url -OutFile $destination
    $actualHash = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actualHash -ne $expectedHash) { throw "Upstream SHA256 mismatch: $relative" }
    $manifest += [ordered]@{path=$relative;sha256=$actualHash;url=$url}
}
& $gitCommand -c core.autocrlf=false -c core.eol=lf -C $sourceRoot apply --check $patch
if ($LASTEXITCODE -ne 0) { throw 'The MSE patch does not apply to the pinned source.' }
& $gitCommand -c core.autocrlf=false -c core.eol=lf -C $sourceRoot apply $patch
if ($LASTEXITCODE -ne 0) { throw 'Applying the MSE patch failed.' }
if ((Get-FileHash -LiteralPath (Join-Path $sourceDirectory 'gstmsesrc.c') -Algorithm SHA256).Hash.ToLowerInvariant() -ne $patchedSourceHash) {
    throw 'The patched source differs from the reviewed version.'
}

$publicHeaders = @('gstmediasource.h','gstsourcebuffer.h','gstsourcebufferlist.h','gstmsesrc.h','mse.h','mse-prelude.h')
$privateHeaders = @('gstappendpipeline-private.h','gstmediasourcesamplemap-private.h','gstmediasourcetrackbuffer-private.h','gstmediasourcetrack-private.h','gstmseeventqueue-private.h','gstmselogging-private.h','gstmsemediatype-private.h')
$headerPrefix = "#pragma once`n#include <glib-object.h>`n#include <gst/mse/mse-prelude.h>`nG_BEGIN_DECLS`n"
$headerEnum = @'
DECORATOR
GType @enum_name@_get_type (void);
#define @ENUMPREFIX@_TYPE_@ENUMSHORT@ (@enum_name@_get_type())
'@
$valueHead = @'
GType
@enum_name@_get_type (void)
{
  static gsize type_id = 0;
  if (g_once_init_enter (&type_id)) {
    static const G@Type@Value values[] = {
'@
$valueTail = @'
      { 0, NULL, NULL }
    };
    GType registered = g_@type@_register_static (g_intern_static_string ("@EnumName@"), values);
    g_once_init_leave (&type_id, registered);
  }
  return (GType) type_id;
}
'@
Push-Location $sourceDirectory
try {
    foreach ($enum in @(@{name='mse-enumtypes';headers=$publicHeaders;decorator='GST_MSE_API'},@{name='mse-enumtypes-private';headers=$privateHeaders;decorator='GST_MSE_PRIVATE'})) {
        $name = $enum.name
        $arguments = @($mkenums,'--fhead',$headerPrefix,'--eprod',($headerEnum.Replace('DECORATOR',$enum.decorator) + "`n"),'--ftail',"G_END_DECLS`n",'--output',(Join-Path $sourceDirectory "$name.h")) + $enum.headers
        & $pythonCommand @arguments
        if ($LASTEXITCODE -ne 0) { throw 'Generating enum declarations failed.' }
        $arguments = @($mkenums,'--fhead',("#include `"$name.h`"`n"),'--fprod',"#include `"@filename@`"`n",'--vhead',($valueHead + "`n"),'--vprod',"      { @VALUENAME@, `"@VALUENAME@`", `"@valuenick@`" },`n",'--vtail',($valueTail + "`n"),'--output',(Join-Path $sourceDirectory "$name.c")) + $enum.headers
        & $pythonCommand @arguments
        if ($LASTEXITCODE -ne 0) { throw 'Generating enum definitions failed.' }
    }
} finally { Pop-Location }

function Get-Exports([string[]]$Lines) {
    @($Lines | ForEach-Object { if ($_ -match '^\s+\d+\s+[0-9A-F]+\s+[0-9A-F]+\s+(\S+)\s*$') { $Matches[1] } } | Sort-Object)
}
function Get-Imports([string[]]$Lines) {
    @($Lines | ForEach-Object { if ($_ -match '^\s+([^\s]+\.dll)\s*$') { $Matches[1].ToLowerInvariant() } } | Sort-Object -Unique)
}
$vsWhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$vs = & $vsWhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $vs) { throw 'Visual Studio x64 C++ build tools are required.' }
$vsDevCmd = Join-Path $vs 'Common7/Tools/VsDevCmd.bat'
$originalEnvironment = [Environment]::GetEnvironmentVariables('Process')
try {
    # Capture into memory only: environment values can contain credentials.
    $vsEnvironment = & $env:ComSpec /d /s /c ('"{0}" -no_logo -host_arch=x64 -arch=x64 >nul && set' -f $vsDevCmd)
    if ($LASTEXITCODE -ne 0) { throw 'VsDevCmd failed.' }
    foreach ($line in $vsEnvironment) {
        if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($Matches[1],$Matches[2],'Process') }
    }
    $compilerVersion = (Get-Item -LiteralPath (Get-Command cl.exe).Source).VersionInfo.FileVersion
    $arguments = @('/nologo','/LD','/MD','/O2','/std:c11','/W3','/DBUILDING_GST_MSE','/DGST_USE_UNSTABLE_API','/D_CRT_SECURE_NO_WARNINGS',('/Fo' + $build + '\'))
    foreach ($include in @($sourceRoot,(Join-Path $sdk 'include/gstreamer-1.0'),(Join-Path $sdk 'include/glib-2.0'),(Join-Path $sdk 'lib/glib-2.0/include'))) { $arguments += '/I' + $include }
    $arguments += @(Get-ChildItem -LiteralPath $sourceDirectory -Filter '*.c' | Sort-Object Name | ForEach-Object Name)
    $dll = Join-Path $build 'gstmse-1.0-0.dll'
    $arguments += @('/link','/Brepro',('/LIBPATH:' + (Join-Path $sdk 'lib')),('/DEF:' + (Join-Path $sdk 'lib/gstmse-1.0.def')),('/OUT:' + $dll),('/IMPLIB:' + (Join-Path $build 'gstmse-1.0.lib')),'gstapp-1.0.lib','gstbase-1.0.lib','gstreamer-1.0.lib','gobject-2.0.lib','glib-2.0.lib')
    Push-Location $sourceDirectory
    try { & cl.exe @arguments } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "Native MSE build failed: $LASTEXITCODE" }
    $baseline = Join-Path $sdk 'bin/gstmse-1.0-0.dll'
    $exports = & dumpbin.exe /nologo /exports $dll
    $baselineExports = & dumpbin.exe /nologo /exports $baseline
    $imports = & dumpbin.exe /nologo /imports $dll
    $baselineImports = & dumpbin.exe /nologo /imports $baseline
    $newExports = @(Get-Exports $exports)
    $oldExports = @(Get-Exports $baselineExports)
    $newImports = @(Get-Imports $imports)
    $oldImports = @(Get-Imports $baselineImports)
    if ($oldExports.Count -ne 51 -or @(Compare-Object $oldExports $newExports).Count) { throw 'The native export ABI differs from the SDK.' }
    if (@(Compare-Object $oldImports $newImports).Count) { throw 'The import DLL set differs from the SDK; review dependencies before use.' }
    $exports | Set-Content -LiteralPath (Join-Path $build 'exports.txt')
    $imports | Set-Content -LiteralPath (Join-Path $build 'imports.txt')
} finally {
    foreach ($key in @([Environment]::GetEnvironmentVariables('Process').Keys)) {
        if (-not $originalEnvironment.Contains($key)) { [Environment]::SetEnvironmentVariable($key,$null,'Process') }
    }
    foreach ($key in $originalEnvironment.Keys) { [Environment]::SetEnvironmentVariable($key,$originalEnvironment[$key],'Process') }
}

$sourceVendor = Join-Path $sourceRoot 'vendor/gstreamer'
New-Item -ItemType Directory -Path $sourceVendor -Force | Out-Null
Copy-Item -LiteralPath $patch -Destination (Join-Path $sourceVendor $patchName)
Copy-Item -LiteralPath $PSCommandPath -Destination (Join-Path $sourceRoot 'Build-MediaPatch.ps1')
[ordered]@{version='1.28.8';commit=$commit;files=$manifest;patch_sha256=$patchHash;patched_gstmsesrc_sha256=$patchedSourceHash} | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $sourceRoot 'upstream-manifest.json') -Encoding utf8
@"
GStreamer MSE initialization patch, based on GStreamer 1.28.8.
Upstream commit: $commit
Upstream source/license: https://github.com/GStreamer/gstreamer/tree/$commit/subprojects/gst-plugins-bad
Original source files retain copyright and LGPL notices; see COPYING.
Changes: release the incomplete-initialization lock; defer first stream publication
until all initial SourceBuffers are ready; release the buffer-list reference.
SourceBuffer remove/abort remain unsupported by this patch. Experimental MSE
must stay disabled until the complete browser-level feature is verified.
Build: powershell -File Build-MediaPatch.ps1 -SdkRoot <MSVC-SDK-1.28.8> -Python <python3-executable> -WorkDirectory <scratch> -OutputDirectory <separate-output>
The build fetches SHA256-pinned upstream source, applies vendor/gstreamer/$patchName,
generates enums with the SDK's glib-mkenums, then compiles only this library using
VS x64 C++ tools. Generated source is included for inspection. No SDK/app file is
modified. DLL loading/replacement is a separate explicitly selected packaging step.
"@ | Set-Content -LiteralPath (Join-Path $sourceRoot 'BUILD-SOURCE.txt') -Encoding utf8
$sourceArchive = Join-Path $build 'gstreamer-mse-1.28.8-rustbrowser-source.zip'
Compress-Archive -LiteralPath $sourceRoot -DestinationPath $sourceArchive
$hash = (Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash.ToLowerInvariant()
$proof = [ordered]@{
    upstream_version='1.28.8';upstream_commit=$commit;patch_sha256=$patchHash;dll_sha256=$hash
    baseline_dll_sha256=(Get-FileHash -LiteralPath $baseline -Algorithm SHA256).Hash.ToLowerInvariant()
    source_archive_sha256=(Get-FileHash -LiteralPath $sourceArchive -Algorithm SHA256).Hash.ToLowerInvariant()
    compiler_version=$compilerVersion;exports=$newExports;import_dlls=$newImports
    exports_match_sdk=$true;import_dlls_match_sdk=$true;experimental_mse_default=$false
}
$proof | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $build 'media-patch-build.json') -Encoding utf8
foreach ($name in @('gstmse-1.0-0.dll','gstreamer-mse-1.28.8-rustbrowser-source.zip','media-patch-build.json','exports.txt','imports.txt')) {
    Copy-Item -LiteralPath (Join-Path $build $name) -Destination (Join-Path $output $name)
}
Copy-Item -LiteralPath (Join-Path $sourceRoot 'COPYING') -Destination (Join-Path $output 'COPYING.gstreamer-mse')
Write-Output (Join-Path $output 'gstmse-1.0-0.dll')
