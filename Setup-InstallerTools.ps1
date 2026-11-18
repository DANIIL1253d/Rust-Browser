param([string]$WorkDirectory)
$ErrorActionPreference = 'Stop'
$taskWork = if ($WorkDirectory) { [IO.Path]::GetFullPath($WorkDirectory) } else { & (Join-Path $PSScriptRoot 'Get-BrowserWork.ps1') -ProjectRoot $PSScriptRoot }
$toolsDirectory = Join-Path $taskWork 'installer-tools'
$version = '7.1.1'
$expectedHash = '8a5a6711c3766a6e5fb74f280ba0e3551e719ce94538d901fdbd1f8f9920b3f0'
$installer = Join-Path $toolsDirectory "innosetup-$version-x64.exe"
$compilerDirectory = Join-Path $toolsDirectory "innosetup-$version"
$compiler = Join-Path $compilerDirectory 'ISCC.exe'
New-Item -ItemType Directory -Path $toolsDirectory -Force | Out-Null
if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) {
    Invoke-WebRequest -Uri "https://github.com/jrsoftware/issrc/releases/download/is-7_1_1/innosetup-$version-x64.exe" -OutFile $installer
}
if ((Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash -ne $expectedHash) { throw 'Inno Setup download SHA256 mismatch.' }
$signature = Get-AuthenticodeSignature -LiteralPath $installer
if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch '(^|,\s*)CN=Pyrsys B\.V\.(,|$)') { throw 'Inno Setup download has an unexpected Authenticode signature.' }
if (-not (Test-Path -LiteralPath $compiler -PathType Leaf)) {
    # Inno Setup's own portable mode stays inside task work and registers no
    # compiler installation, shortcuts, file associations or uninstaller.
    $arguments = @('/PORTABLE=1', '/CURRENTUSER', '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOICONS', ('/DIR="' + $compilerDirectory + '"'))
    $process = Start-Process -FilePath $installer -ArgumentList $arguments -WindowStyle Hidden -PassThru -Wait
    if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $compiler -PathType Leaf)) { throw "Portable Inno Setup extraction failed: $($process.ExitCode)." }
}
$compilerSignature = Get-AuthenticodeSignature -LiteralPath $compiler
if ($compilerSignature.Status -ne 'Valid' -or $compilerSignature.SignerCertificate.Subject -notmatch '(^|,\s*)CN=Pyrsys B\.V\.(,|$)') { throw 'Unexpected Inno Setup compiler Authenticode signature.' }
Write-Output $compiler
