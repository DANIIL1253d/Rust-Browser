param([string]$WorkDirectory)
$ErrorActionPreference = 'Stop'
$projectRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$taskWork = & (Join-Path $projectRoot 'Get-BrowserWork.ps1') -ProjectRoot $projectRoot
$work = if ($WorkDirectory) { [IO.Path]::GetFullPath($WorkDirectory) } else { Join-Path $taskWork 'installer-validation' }
New-Item -ItemType Directory -Path $work -Force | Out-Null
$buildScript = Join-Path $projectRoot 'Build-Installer.ps1'
if (-not (Test-Path -LiteralPath $buildScript)) { throw 'FAIL: a valid release must be accepted by Build-Installer.ps1; the feature is missing.' }

Add-Type -AssemblyName System.IO.Compression
$required = @(
    'app/RustBrowser.exe', 'app/LICENSE-Servo.txt', 'app/resources/test.txt',
    'app/runtime/sing-box.exe', 'app/runtime/xray.exe', 'app/runtime/LICENSE-sing-box.txt',
    'app/runtime/COPYING-GPL-3.0.txt', 'app/runtime/LICENSE-Xray.txt', 'app/runtime/THIRD-PARTY.txt',
    'Start-Browser.cmd', 'README-BROWSER.md', 'RELEASE-NOTES.md', 'CONTRIBUTORS.md', 'LICENSE',
    'design/THIRD-PARTY.md', 'design/LICENSE-Lucide.txt', 'design/LICENSE-Unslop.txt', 'licenses/CSV-MIT.txt'
)
function New-Fixture([string]$Name, [string]$Mutation) {
    $files = [ordered]@{}
    foreach ($relative in $required) { $files[$relative] = [Text.Encoding]::UTF8.GetBytes("fixture:$relative") }
    if ($Mutation -eq 'missing-license') { $files.Remove('app/runtime/COPYING-GPL-3.0.txt') }
    if ($Mutation -eq 'traversal') { $files['../outside.txt'] = [Text.Encoding]::UTF8.GetBytes('outside') }
    $hashes = [ordered]@{}
    foreach ($relative in $files.Keys) { $hashes[$relative] = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($files[$relative])).ToLowerInvariant() }
    $manifest = @{ version = $(if ($Mutation -eq 'version') { 'v9.9.9' } else { 'v0.1.0' }); platform = 'windows-x86_64-msvc'; source_commit = ('1' * 40); sha256 = $hashes }
    if ($Mutation -eq 'tamper') { $files['app/RustBrowser.exe'] = [Text.Encoding]::UTF8.GetBytes('tampered') }
    if ($Mutation -eq 'unlisted') { $files['unlisted.txt'] = [Text.Encoding]::UTF8.GetBytes('extra') }
    $files['release-manifest.json'] = [Text.Encoding]::UTF8.GetBytes(($manifest | ConvertTo-Json -Depth 5))
    $archive = Join-Path $work ($Name + '-' + [guid]::NewGuid().ToString('N') + '.zip')
    $stream = [IO.File]::Open($archive, [IO.FileMode]::CreateNew)
    $zip = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($relative in $files.Keys) {
            $entry = $zip.CreateEntry("RustBrowser-v0.1.0-windows-x64/$relative")
            $output = $entry.Open()
            try { $output.Write($files[$relative]) } finally { $output.Dispose() }
        }
        if ($Mutation -eq 'duplicate') {
            $output = $zip.CreateEntry('RustBrowser-v0.1.0-windows-x64/APP/RustBrowser.exe').Open()
            try { $output.Write($files['app/RustBrowser.exe']) } finally { $output.Dispose() }
        }
    } finally { $zip.Dispose(); $stream.Dispose() }
    return $archive
}

$cases = @(
    @('valid', '', ''),
    @('archive-checksum', '', 'Archive SHA256 mismatch'),
    @('tampered-file', 'tamper', 'Payload SHA256 mismatch'),
    @('unlisted-file', 'unlisted', 'not listed in the manifest'),
    @('missing-license', 'missing-license', 'Missing required release file'),
    @('path-traversal', 'traversal', 'Unsafe archive path'),
    @('wrong-version', 'version', 'Manifest version'),
    @('duplicate-case', 'duplicate', 'Duplicate archive path')
)
foreach ($case in $cases) {
    $archive = New-Fixture $case[0] $case[1]
    $digest = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
    if ($case[0] -eq 'archive-checksum') { $digest = '0' * 64 }
    $failure = $null
    $result = $null
    try { $result = & $buildScript -Version v0.1.0 -ArchivePath $archive -ArchiveSha256 $digest -ValidateOnly -WorkDirectory $work }
    catch { $failure = $_.Exception.Message }
    if (-not $case[2]) {
        if ($failure -or $result.Files -ne ($required.Count + 1)) { throw "FAIL: $($case[0]): $failure" }
    } elseif (-not $failure -or $failure -notmatch [regex]::Escape($case[2])) { throw "FAIL: $($case[0]): expected '$($case[2])', got '$failure'" }
    Write-Output "PASS: $($case[0])"
}
