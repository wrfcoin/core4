; WRFCoin Node for Windows - Inno Setup 6 script (per-user, no admin).
;
; Built in CI by .github/workflows/windows-node-build.yml:
;   ISCC.exe /DAppVersion=<ver> /DSourceExe=<path\launch_api.exe> /DSourceCli=<path\wrfcoin.exe> /DOutputDir=<dir> wrfcoin-node.iss
; Installs launch_api.exe, the wrfcoin.exe operator CLI, the chain-888 sync-node config and the launcher to
; %LOCALAPPDATA%\Programs\WRFCoin Node. Node data goes to %LOCALAPPDATA%\WRFCoin
; (created and locked to the current user by wrfcoin-node.ps1 -Action Setup).

#ifndef AppVersion
  #define AppVersion "0.0.0-dev"
#endif
#ifndef SourceExe
  #define SourceExe "..\..\target\release\launch_api.exe"
#endif
#ifndef SourceCli
  #define SourceCli "..\..\target\release\wrfcoin.exe"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\target\installer"
#endif

#define AppName "WRFCoin Node"
#define StatusUrl "http://127.0.0.1:8556/dashboard"
#define Launcher "{sys}\WindowsPowerShell\v1.0\powershell.exe"
; -WindowStyle Hidden: the user sees no console window (a brief flash at most).
; A VBScript shim would hide even that, but VBScript is being retired from
; Windows. Single-quoted ISPP string: the doubled "" survive into the entry
; parameters below, where Inno reads "" as one quote around the path.
#define LauncherArgs '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File ""{app}\wrfcoin-node.ps1""'

[Setup]
AppId={{408EB92E-F98C-47E6-AFBF-1A4226024AE5}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion} (chain 888)
AppPublisher=WRFCoin
AppPublisherURL=https://github.com/wrfcoin/core4
; Per-user: never asks for administrator rights; {autopf} resolves to
; %LOCALAPPDATA%\Programs and shortcuts go to the user's own Start menu.
PrivilegesRequired=lowest
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=wrfcoin-node-{#AppVersion}-windows-x86_64-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\launch_api.exe
CloseApplications=yes
RestartApplications=no

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "launch_api.exe"; Flags: ignoreversion
Source: "{#SourceCli}"; DestDir: "{app}"; DestName: "wrfcoin.exe"; Flags: ignoreversion
Source: "chain-888.conf"; DestDir: "{app}"; Flags: ignoreversion
Source: "wrfcoin-node.ps1"; DestDir: "{app}"; Flags: ignoreversion

[INI]
Filename: "{app}\WRFCoin Node Status.url"; Section: "InternetShortcut"; Key: "URL"; String: "{#StatusUrl}"

[Tasks]
Name: "startup"; Description: "Start the WRFCoin node when I sign in to Windows"; Flags: unchecked

[Icons]
Name: "{group}\WRFCoin Node"; Filename: "{#Launcher}"; Parameters: "{#LauncherArgs} -Action Start"; WorkingDir: "{app}"; IconFilename: "{app}\launch_api.exe"; Comment: "Start the WRFCoin sync node (chain 888) and open its status page"
Name: "{group}\Stop WRFCoin Node"; Filename: "{#Launcher}"; Parameters: "{#LauncherArgs} -Action Stop"; WorkingDir: "{app}"; IconFilename: "{app}\launch_api.exe"; Comment: "Stop the WRFCoin node cleanly"
Name: "{group}\WRFCoin Node Status"; Filename: "{app}\WRFCoin Node Status.url"; Comment: "Open the node's status page in your browser"
Name: "{group}\WRFCoin Node Data Folder"; Filename: "{localappdata}\WRFCoin"; Comment: "Chain data, identity key and logs"
Name: "{group}\Uninstall WRFCoin Node"; Filename: "{uninstallexe}"
Name: "{userstartup}\WRFCoin Node"; Filename: "{#Launcher}"; Parameters: "{#LauncherArgs} -Action Start -NoBrowser"; WorkingDir: "{app}"; IconFilename: "{app}\launch_api.exe"; Tasks: startup

[Run]
; Create %LOCALAPPDATA%\WRFCoin, lock it to this user and create the operator token.
Filename: "{#Launcher}"; Parameters: "{#LauncherArgs} -Action Setup -Quiet"; Flags: runhidden waituntilterminated; StatusMsg: "Preparing the node data folder..."
Filename: "{#Launcher}"; Parameters: "{#LauncherArgs} -Action Start"; WorkingDir: "{app}"; Description: "Start the WRFCoin node now"; Flags: postinstall nowait skipifsilent

[UninstallRun]
; Stop a running node cleanly (Ctrl+C on its hidden console) before its files go.
Filename: "{#Launcher}"; Parameters: "{#LauncherArgs} -Action Stop -Quiet"; Flags: runhidden waituntilterminated; RunOnceId: "StopNode"

[UninstallDelete]
Type: files; Name: "{app}\WRFCoin Node Status.url"

[Code]
// Upgrading over a running node: stop it cleanly first, so its files can be
// replaced without killing it mid-checkpoint.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Script: String;
  ResultCode: Integer;
begin
  Result := '';
  Script := ExpandConstant('{app}\wrfcoin-node.ps1');
  if FileExists(Script) then
    Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
         '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "' + Script + '" -Action Stop -Quiet',
         '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataRoot: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    DataRoot := ExpandConstant('{localappdata}\WRFCoin');
    if DirExists(DataRoot) and not UninstallSilent then
      if MsgBox('Also delete the node''s chain data and identity key in ' + DataRoot + '?' + #13#10#13#10 +
                'Choose No to keep them (a reinstall then resumes where it stopped).',
                mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
        DelTree(DataRoot, True, True, True);
  end;
end;
