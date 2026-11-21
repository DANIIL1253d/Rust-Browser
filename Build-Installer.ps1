param(
    [Parameter(Mandatory=$true)][ValidatePattern('^v[0-9]+\.[0-9]+\.[0-9]+(?:-[a-zA-Z0-9.-]+)?$')][string]$Version,
    [string]$ArchivePath,
    [ValidatePattern('^[a-fA-F0-9]{64}$')][string]$ArchiveSha256,
    [string]$OutputDirectory,
    [string]$WorkDirectory,
    [switch]$ValidateOnly
)
$ErrorActionPreference = 'Stop'
$taskWork = if ($WorkDirectory) { [IO.Path]::GetFullPath($WorkDirectory) } else { & (Join-Path $PSScriptRoot 'Get-BrowserWork.ps1') -ProjectRoot $PSScriptRoot }
$bundleName = "RustBrowser-$Version-windows-x64"
$archive = if ($ArchivePath) { [IO.Path]::GetFullPath($ArchivePath) } else { Join-Path $PSScriptRoot "releases\$Version\$bundleName.zip" }
if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) { throw "Release archive not found: $archive" }
if (-not $ArchiveSha256) {
    $checksums = Join-Path (Split-Path -Parent $archive) 'SHA256SUMS.txt'
    if (-not (Test-Path -LiteralPath $checksums -PathType Leaf)) { throw 'Pass -ArchiveSha256 or supply the release SHA256SUMS.txt next to the ZIP.' }
    $pattern = '^([a-fA-F0-9]{64})\s+\*?' + [regex]::Escape([IO.Path]::GetFileName($archive)) + '$'
    $matches = @(Get-Content -LiteralPath $checksums | Where-Object { $_ -match $pattern })
    if ($matches.Count -ne 1) { throw 'Release checksum file must name the archive exactly once.' }
    $ArchiveSha256 = [regex]::Match($matches[0], $pattern).Groups[1].Value
}
$archiveHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($archiveHash -ne $ArchiveSha256) { throw 'Archive SHA256 mismatch.' }

Add-Type -AssemblyName System.IO.Compression
$stage = Join-Path $taskWork ('installer-stage-' + [guid]::NewGuid().ToString('N'))
$payload = Join-Path $stage $bundleName
New-Item -ItemType Directory -Path $payload -Force | Out-Null
$stream = [IO.File]::OpenRead($archive)
$zip = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Read)
try {
    $entries = [Collections.Generic.Dictionary[string,object]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($entry in $zip.Entries) {
        $prefix = "$bundleName/"
        if (-not $entry.FullName.StartsWith($prefix, [StringComparison]::Ordinal)) { throw "Unsafe archive path: $($entry.FullName)" }
        $relative = $entry.FullName.Substring($prefix.Length)
        # Reject traversal, alternate Windows streams, duplicate names and
        # normalized aliases before writing any archive content to disk.
        if (-not $relative -or $relative.Contains('\') -or $relative -match '[:\x00-\x1f<>"|?*]' -or
            @($relative.Split('/') | Where-Object { -not $_ -or $_ -in @('.', '..') -or $_ -match '[. ]$' -or $_ -match '^(?i:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)' }).Count) {
            throw "Unsafe archive path: $($entry.FullName)"
        }
        if ($entries.ContainsKey($relative)) { throw "Duplicate archive path: $relative" }
        $entries.Add($relative, $entry)
    }
    if (-not $entries.ContainsKey('release-manifest.json')) { throw 'Release manifest is missing.' }
    $reader = [IO.StreamReader]::new($entries['release-manifest.json'].Open())
    try { $manifest = $reader.ReadToEnd() | ConvertFrom-Json } finally { $reader.Dispose() }
    if ($manifest.version -cne $Version) { throw 'Manifest version does not match -Version.' }
    if ($manifest.platform -cne 'windows-x86_64-msvc' -or $manifest.source_commit -notmatch '^[a-fA-F0-9]{40}$') { throw 'Manifest platform or source commit is invalid.' }
    if (-not $manifest.sha256 -or $manifest.sha256 -is [string]) { throw 'Manifest SHA256 map is missing.' }
    $hashes = [Collections.Generic.Dictionary[string,string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($property in $manifest.sha256.PSObject.Properties) {
        if ($property.Value -isnot [string] -or $property.Value -notmatch '^[a-fA-F0-9]{64}$' -or $hashes.ContainsKey($property.Name)) { throw 'Manifest SHA256 map is invalid.' }
        $hashes.Add($property.Name, $property.Value)
    }
    foreach ($relative in $entries.Keys) {
        if ($relative -cne 'release-manifest.json' -and -not $hashes.ContainsKey($relative)) { throw "Archive file is not listed in the manifest: $relative" }
    }
    foreach ($relative in $hashes.Keys) {
        if ($relative -ieq 'release-manifest.json' -or -not $entries.ContainsKey($relative)) { throw "Manifest file is missing from the archive: $relative" }
    }
    foreach ($required in @(
        'app/RustBrowser.exe', 'app/LICENSE-Servo.txt',
        'app/runtime/sing-box.exe', 'app/runtime/xray.exe', 'app/runtime/LICENSE-sing-box.txt',
        'app/runtime/COPYING-GPL-3.0.txt', 'app/runtime/LICENSE-Xray.txt', 'app/runtime/THIRD-PARTY.txt',
        'Start-Browser.cmd', 'README-BROWSER.md', 'RELEASE-NOTES.md', 'CONTRIBUTORS.md', 'LICENSE',
        'design/THIRD-PARTY.md', 'design/LICENSE-Lucide.txt', 'design/LICENSE-Unslop.txt', 'licenses/CSV-MIT.txt'
    )) {
        if (-not $hashes.ContainsKey($required)) { throw "Missing required release file: $required" }
    }
    if (-not @($hashes.Keys | Where-Object { $_.StartsWith('app/resources/', [StringComparison]::Ordinal) }).Count) { throw 'Missing required release files: app/resources.' }
    foreach ($relative in $entries.Keys) {
        $destination = Join-Path $payload $relative
        New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
        $input = $entries[$relative].Open()
        $output = [IO.File]::Open($destination, [IO.FileMode]::CreateNew)
        try { $input.CopyTo($output) } finally { $output.Dispose(); $input.Dispose() }
        if ($relative -cne 'release-manifest.json' -and (Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -ne $hashes[$relative]) { throw "Payload SHA256 mismatch: $relative" }
    }
    $zip.Dispose()
    $stream.Dispose()
    if ($ValidateOnly) {
        [pscustomobject]@{ Version = $Version; Files = $entries.Count; ArchiveSha256 = $archiveHash }
        return
    }
    $releaseDir = if ($OutputDirectory) { [IO.Path]::GetFullPath($OutputDirectory) } else { Split-Path -Parent $archive }
    $installerName = "$bundleName-setup"
    $installer = Join-Path $releaseDir "$installerName.exe"
    if (Test-Path -LiteralPath $installer) { throw 'Installer output already exists; choose an empty output directory.' }
    New-Item -ItemType Directory -Path $releaseDir -Force | Out-Null
    $compiler = & (Join-Path $PSScriptRoot 'Setup-InstallerTools.ps1') -WorkDirectory $taskWork
    $numericVersion = ($Version.Substring(1) -split '-')[0]
    & $compiler "/DPayloadDir=$payload" "/DOutputDir=$releaseDir" "/DInstallerName=$installerName" "/DProductVersion=$numericVersion" "/DReleaseVersion=$Version" (Join-Path $PSScriptRoot 'installer\browser.iss')
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $installer -PathType Leaf)) { throw 'Inno Setup compiler failed.' }
    $installerHash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText((Join-Path $releaseDir "$installerName.sha256"), "$installerHash  $installerName.exe`n", [Text.UTF8Encoding]::new($false))
    Write-Output $installer
} finally {
    $zip.Dispose()
    $stream.Dispose()
    # $stage is created above from our GUID, never from archive content.
    $resolvedStage = [IO.Path]::GetFullPath($stage)
    $resolvedWork = [IO.Path]::GetFullPath($taskWork).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    if (-not $resolvedStage.StartsWith($resolvedWork, [StringComparison]::OrdinalIgnoreCase)) { throw 'Refusing cleanup outside installer work.' }
    Remove-Item -LiteralPath $resolvedStage -Recurse -Force
}
