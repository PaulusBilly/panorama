; Inno Setup 6 script for Panorama on Windows x64. Built by scripts/package-windows-installer.ps1,
; which passes /DAppVersion, /DSourceDir (the verified package folder) and /DOutputDir.
; Installs per user, so no administrator prompt; libmpv-2.dll always lands beside panorama.exe.

#ifndef AppVersion
  #error AppVersion is not set. Build with scripts/package-windows-installer.ps1.
#endif
#ifndef SourceDir
  #error SourceDir is not set. Build with scripts/package-windows-installer.ps1.
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

[Setup]
; Never change AppId: Windows uses it to find an installed Panorama to upgrade or remove.
AppId={{E2338C44-139B-4BF7-B65D-E17AC8E99896}
AppName=Panorama
AppVersion={#AppVersion}
AppVerName=Panorama {#AppVersion}
AppPublisher=Panorama
DefaultDirName={autopf}\Panorama
DefaultGroupName=Panorama
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=Panorama-Setup-{#AppVersion}
SetupIconFile=panorama.ico
UninstallDisplayIcon={app}\panorama.ico
UninstallDisplayName=Panorama
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
; Upgrading while Panorama is open closes it first, so panorama.exe and libmpv-2.dll are not
; left half replaced.
CloseApplications=force
RestartApplications=no

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "panorama.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Panorama"; Filename: "{app}\panorama.exe"; IconFilename: "{app}\panorama.ico"
Name: "{autodesktop}\Panorama"; Filename: "{app}\panorama.exe"; IconFilename: "{app}\panorama.ico"; Tasks: desktopicon

[Registry]
; panorama:// links (watch party invites) open Panorama, which joins the party. Per user, so no
; administrator prompt, and removed on uninstall.
Root: HKCU; Subkey: "Software\Classes\panorama"; ValueType: string; ValueName: ""; ValueData: "URL:Panorama watch party"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\panorama"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\panorama\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\panorama.ico"
Root: HKCU; Subkey: "Software\Classes\panorama\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\panorama.exe"" --open-url ""%1"""

[Run]
Filename: "{app}\panorama.exe"; Description: "Open Panorama"; Flags: nowait postinstall skipifsilent

; The account, settings and caches in %LOCALAPPDATA%\Panorama are the user's and stay after an
; uninstall, so reinstalling keeps them signed in.
