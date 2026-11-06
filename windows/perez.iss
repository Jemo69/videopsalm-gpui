; Perez RS — Windows installer (Inno Setup 6).
; Wraps the CI-built portable exe (perez-rs-windows-x64.exe) into a
; Start-Menu + desktop install. Build with: iscc windows/perez.iss
; (run from the repository root so the Files source paths resolve).

#define MyAppName "Perez RS"
#define MyAppExe "perez-rs.exe"
#define MyAppVersion GetEnv("PEREZ_VERSION") == "" ? "0.1.19" : GetEnv("PEREZ_VERSION")

[Setup]
AppId={{3F6A1B2C-9D4E-4F8A-B7C1-2E5D6A8B0C3D}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher=Perez
DefaultDirName={autopf}\Perez RS
DefaultGroupName=Perez RS
OutputBaseFilename=Perez-Setup-{#MyAppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayName=Perez RS
DisableProgramGroupPage=yes

[Files]
; CI artifact renamed to the stable exe name before running iscc:
;   copy perez-rs-windows-x64.exe dist\perez-rs.exe
Source: "dist\{#MyAppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "assets\*"; DestDir: "{app}\assets"; Flags: ignoreversion recursesubdirs createallsubdirs skipifsourcedoesntexist
Source: "locales\*"; DestDir: "{app}\locales"; Flags: ignoreversion recursesubdirs createallsubdirs skipifsourcedoesntexist
Source: "bibles\*"; DestDir: "{app}\bibles"; Flags: ignoreversion recursesubdirs createallsubdirs skipifsourcedoesntexist

[Icons]
Name: "{autoprograms}\Perez RS"; Filename: "{app}\{#MyAppExe}"
Name: "{autodesktop}\Perez RS"; Filename: "{app}\{#MyAppExe}"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop icon"; GroupDescription: "Additional icons:"

[Run]
Filename: "{app}\{#MyAppExe}"; Description: "Launch Perez RS"; Flags: nowait postinstall skipifsilent
