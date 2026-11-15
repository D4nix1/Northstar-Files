#define AppName "Northstar Files"
#define AppVersion "0.1.0"
#define AppExe "northstar-files.exe"

[Setup]
AppId={{627BD8EF-B5A3-4C46-8C60-442EDB111557}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Northstar Files Community
AppPublisherURL=https://github.com/
DefaultDirName={autopf}\Northstar Files
DefaultGroupName=Northstar Files
DisableProgramGroupPage=yes
OutputDir=..\..\..\dist
OutputBaseFilename=Northstar-Files-Setup
SetupIconFile=..\assets\northstar.ico
UninstallDisplayIcon={app}\{#AppExe}
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
LicenseFile=..\..\..\LICENSE-MIT
Uninstallable=yes

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Criar atalho na área de trabalho"; GroupDescription: "Atalhos:"; Flags: checkedonce

[Files]
Source: "..\..\..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\northstar.ico"; DestDir: "{app}\assets"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Northstar Files"; Filename: "{app}\{#AppExe}"; IconFilename: "{app}\assets\northstar.ico"
Name: "{autodesktop}\Northstar Files"; Filename: "{app}\{#AppExe}"; IconFilename: "{app}\assets\northstar.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "Abrir Northstar Files"; Flags: postinstall nowait skipifsilent
