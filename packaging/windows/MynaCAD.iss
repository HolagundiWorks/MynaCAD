; MynaCAD Setup: one installer for MynaCAD on its own (per user, no administrator).
; Installs the portable layout (ODraft.WinUi.exe, o-draft.exe, aad-cli.exe and their files) under %LocalAppData%\Programs\O-Draft,
; adds a Start menu shortcut, and makes a double-click on a project file (.osp) open it in MynaCAD.
; The folder keeps the old name O-Draft on purpose: it is an identifier, so earlier installs are upgraded in place.
; Build with installer\suite\build-all.ps1 in the MynaHub repository (it unpacks the portable zip into the stage folder and calls ISCC),
; or by hand:  ISCC /DAppVersion=1.7.0 /DStage=<folder with ODraft.WinUi.exe> /DOutDir=<folder> MynaCAD.iss

#ifndef AppVersion
  #define AppVersion "1.0.0"
#endif
#ifndef Stage
  #define Stage "stage"
#endif
#ifndef OutDir
  #define OutDir "out"
#endif
#define MyAppName "MynaCAD"
#define MyPublisher "Human Centric Works"

[Setup]
AppId={{9C1B7E52-6D4A-4B0F-A3E8-2F5D7C1A9B64}
AppName={#MyAppName}
AppVersion={#AppVersion}
AppPublisher={#MyPublisher}
DefaultDirName={localappdata}\Programs\O-Draft
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
UsePreviousAppDir=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutDir}
OutputBaseFilename=MynaCAD-Setup-{#AppVersion}
Compression=lzma2/fast
SolidCompression=no
WizardStyle=modern
LicenseFile={#Stage}\LICENSE
UninstallDisplayName={#MyAppName}
UninstallDisplayIcon={app}\ODraft.WinUi.exe
VersionInfoVersion={#AppVersion}.0
VersionInfoCompany={#MyPublisher}
VersionInfoProductName={#MyAppName}
VersionInfoDescription={#MyAppName} Setup
CloseApplications=yes
ChangesAssociations=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "{#Stage}\app\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[InstallDelete]
; Shortcuts made by earlier Setups (the suite Setup and the names Tren and O-Draft)
Type: files; Name: "{userprograms}\O-Draft.lnk"
Type: files; Name: "{userprograms}\Tren.lnk"
Type: files; Name: "{userprograms}\MynaCAD.lnk"
Type: files; Name: "{userdesktop}\Tren.lnk"

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\ODraft.WinUi.exe"; WorkingDir: "{app}"
Name: "{userdesktop}\{#MyAppName}"; Filename: "{app}\ODraft.WinUi.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Registry]
; The Myna project file (.osp): double-click opens the file in MynaCAD. MynaOps's own Setup adds "Open in MynaOps". Neither verb starts Myna.
Root: HKCU; Subkey: "Software\Classes\.osp"; ValueType: string; ValueName: ""; ValueData: "OSAS.Project"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\OSAS.Project"; ValueType: string; ValueName: ""; ValueData: "Myna project"; Flags: createvalueifdoesntexist
Root: HKCU; Subkey: "Software\Classes\OSAS.Project\shell\open"; ValueType: string; ValueName: ""; ValueData: "Open in MynaCAD"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\OSAS.Project\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\ODraft.WinUi.exe"" ""%1"""

[Run]
Filename: "{app}\ODraft.WinUi.exe"; Description: "Start {#MyAppName}"; Flags: nowait postinstall skipifsilent
