param(
    [Parameter(Mandatory=$true)][ValidatePattern('^v[0-9]+\.[0-9]+\.[0-9]+(?:-[a-zA-Z0-9.-]+)?$')][string]$Version,
    [string]$OutputDirectory,
    [ValidatePattern('^[a-fA-F0-9]{40}$')][string]$SourceRevision
)
$ErrorActionPreference = 'Stop'
$projectRoot = $PSScriptRoot
$taskWork = & (Join-Path $projectRoot 'Get-BrowserWork.ps1') -ProjectRoot $projectRoot
if (-not $SourceRevision) {
    $SourceRevision = & git -C $projectRoot rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Pass the exact source commit using -SourceRevision.' }
}
$releaseDir = if ($OutputDirectory) { [IO.Path]::GetFullPath($OutputDirectory) } else { Join-Path $projectRoot "releases\$Version" }
New-Item -ItemType Directory -Path $releaseDir -Force | Out-Null
$name = "RustBrowser-$Version-windows-x64"
$archive = Join-Path $releaseDir "$name.zip"
$checksums = Join-Path $releaseDir 'SHA256SUMS.txt'
if ((Test-Path -LiteralPath $archive) -or (Test-Path -LiteralPath $checksums)) { throw 'Release output already exists; choose an empty output directory.' }

# Package from build inputs into a fresh directory. Never copy the running
# application's profile, caches, downloads or other user-created files.
$stage = Join-Path $taskWork ('release-stage-' + [guid]::NewGuid().ToString('N'))
$bundle = Join-Path $stage 'app'
& (Join-Path $projectRoot 'Package-Browser.ps1') -AppDirectory $bundle | Out-Null
$files = [ordered]@{}
foreach ($file in Get-ChildItem -LiteralPath $bundle -File) {
    if ($file.Name -eq 'RustBrowser.exe' -or $file.Extension -eq '.dll' -or $file.Name -eq 'LICENSE-Servo.txt') {
        $files["app/$($file.Name)"] = $file.FullName
    }
}
foreach ($file in Get-ChildItem -LiteralPath (Join-Path $bundle 'resources') -File -Recurse) {
    $relative = [IO.Path]::GetRelativePath($bundle, $file.FullName).Replace('\','/')
    if ($relative -match '(^|/)cert-wpt-only/' -or $file.Name -in @('cert-wpt-only','wpt-prefs.json')) { continue }
    $files["app/$relative"] = $file.FullName
}
foreach ($relative in @('runtime/sing-box.exe','runtime/xray.exe','runtime/LICENSE-sing-box.txt','runtime/COPYING-GPL-3.0.txt','runtime/LICENSE-Xray.txt','runtime/THIRD-PARTY.txt')) {
    $file = Join-Path $bundle $relative
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { throw "Missing runtime distribution file: $relative" }
    $files["app/$relative"] = $file
}
foreach ($relative in @('Start-Browser.cmd','README-BROWSER.md','RELEASE-NOTES.md','CONTRIBUTORS.md','LICENSE','design/THIRD-PARTY.md','design/LICENSE-Lucide.txt','design/LICENSE-Unslop.txt','licenses/CSV-MIT.txt')) {
    $files[$relative] = Join-Path $projectRoot $relative
}
$hashes = [ordered]@{}
foreach ($relative in $files.Keys) {
    if ($relative -match '(^|/)(profile\.json|cookies|auth\.json|passwords\.dat|connections\.dat)$' -or $relative -match '\.(sqlite|log|csv)$') { throw 'Private data is not a release input.' }
    $hashes[$relative] = (Get-FileHash -LiteralPath $files[$relative] -Algorithm SHA256).Hash.ToLowerInvariant()
}
$manifest = [ordered]@{
    version = $Version
    platform = 'windows-x86_64-msvc'
    source_commit = $SourceRevision
    servo_revision = 'aac43a3f31a259f04a574f5ec4e959c943ad7cc7'
    build_profile = 'development preview; media backend disabled'
    sha256 = $hashes
} | ConvertTo-Json -Depth 4

Add-Type -AssemblyName System.IO.Compression
$stream = [IO.File]::Open($archive, [IO.FileMode]::CreateNew)
$zip = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create, $false)
try {
    foreach ($relative in $files.Keys) {
        $entry = $zip.CreateEntry("$name/$relative", [IO.Compression.CompressionLevel]::Optimal)
        $entry.LastWriteTime = [DateTimeOffset]::new(2000,1,1,0,0,0,[TimeSpan]::Zero)
        $input = [IO.File]::OpenRead($files[$relative])
        $output = $entry.Open()
        try { $input.CopyTo($output) } finally { $output.Dispose(); $input.Dispose() }
    }
    $entry = $zip.CreateEntry("$name/release-manifest.json")
    $entry.LastWriteTime = [DateTimeOffset]::new(2000,1,1,0,0,0,[TimeSpan]::Zero)
    $writer = [IO.StreamWriter]::new($entry.Open(), [Text.UTF8Encoding]::new($false))
    try { $writer.WriteLine($manifest) } finally { $writer.Dispose() }
} finally { $zip.Dispose(); $stream.Dispose() }
$digest = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText($checksums, "$digest  $name.zip`n", [Text.UTF8Encoding]::new($false))
Write-Output $archive
Write-Output $checksums
