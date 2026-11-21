#ifndef PayloadDir
  #error PayloadDir must come from Build-Installer.ps1's validated release archive
#endif

[Setup]
AppId={code:GetInstallAppId}
AppName=Rust Browser
AppVersion={#ReleaseVersion}
AppPublisher=Rust Browser contributors
VersionInfoVersion={#ProductVersion}
DefaultDirName={localappdata}\Programs\RustBrowser
DefaultGroupName=Rust Browser
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\app\RustBrowser.exe
OutputDir={#OutputDir}
OutputBaseFilename={#InstallerName}
Compression=lzma2/normal
SolidCompression=yes
WizardStyle=modern
CloseApplications=no
RestartApplications=no
SetupLogging=yes
UsePreviousLanguage=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked; Check: IsNormalInstall

[Files]
Source: "{#PayloadDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\Rust Browser"; Filename: "{app}\app\RustBrowser.exe"; WorkingDir: "{app}\app"; Check: IsNormalInstall
Name: "{autodesktop}\Rust Browser"; Filename: "{app}\app\RustBrowser.exe"; WorkingDir: "{app}\app"; Tasks: desktopicon; Check: IsNormalInstall

[Run]
Filename: "{app}\app\RustBrowser.exe"; WorkingDir: "{app}\app"; Description: "{cm:LaunchProgram,Rust Browser}"; Flags: nowait postinstall skipifsilent runasoriginaluser; Check: IsNormalInstall

[Code]
function IsTestInstall: Boolean;
begin
  Result := ExpandConstant('{param:TESTINSTALL|0}') = '1';
end;

function IsNormalInstall: Boolean;
begin
  Result := not IsTestInstall;
end;

function GetInstallAppId(Param: String): String;
begin
  if IsTestInstall then
    Result := 'RustBrowser.InstallerSmoke'
  else
    Result := 'RustBrowser.6BC79C9E-00E1-491B-895B-CC89ED1F6B00';
end;

function InitializeSetup: Boolean;
begin
  Result := True;
  if IsTestInstall and (ExpandConstant('{param:DIR|}') = '') then
  begin
    MsgBox('TESTINSTALL requires an explicit scratch /DIR.', mbError, MB_OK);
    Result := False;
  end;
end;
