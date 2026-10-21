; Builds the porthouse installer. Run through scripts/package-windows.ps1, which prepares the files.

#ifndef Version
  #error Pass the version with /DVersion=x.y.z
#endif
#ifndef Stage
  #error Pass the staging directory with /DStage=path
#endif
#define UsbipInstaller "USBip-0.9.8.1-x64.exe"

[Setup]
AppId={{B2C0D6F4-3E57-4B8E-9A1C-6D2F8E7A5C13}
AppName=Porthouse
AppVersion={#Version}
AppPublisher=porthouse
AppPublisherURL=https://github.com/RisPNG/porthouse
DefaultDirName={autopf}\porthouse
DefaultGroupName=Porthouse
DisableProgramGroupPage=yes
OutputBaseFilename=porthouse-{#Version}-windows-x64-setup
OutputDir={#Stage}\..
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
WizardStyle=modern
UninstallDisplayIcon={app}\porthouse.exe
SetupIconFile={#Stage}\porthouse.ico
CloseApplications=yes
RestartApplications=no
LicenseFile={#Stage}\LICENSE

[Types]
Name: "full"; Description: "Use and share devices"
Name: "client"; Description: "Only use devices shared by other computers"
Name: "server"; Description: "Only share this computer's devices"
Name: "custom"; Description: "Choose what to install"; Flags: iscustom

[Components]
Name: "program"; Description: "The Porthouse program"; Types: full client server custom; Flags: fixed
Name: "client"; Description: "Use devices from other computers (also installs the USB/IP driver)"; Types: full client
Name: "server"; Description: "Share this computer's devices"; Types: full server

[Tasks]
Name: "autostart"; Description: "Show the Porthouse icon in the notification area when I sign in"

[Dirs]
Name: "{commonappdata}\porthouse"

[Files]
Source: "{#Stage}\gui\*"; DestDir: "{app}"; Flags: recursesubdirs ignoreversion; Components: program
Source: "{#Stage}\porthouse-client.exe"; DestDir: "{app}"; Flags: ignoreversion; Components: client
Source: "{#Stage}\porthouse-clientd.exe"; DestDir: "{app}"; Flags: ignoreversion; Components: client
Source: "{#Stage}\porthouse-server.exe"; DestDir: "{app}"; Flags: ignoreversion; Components: server
Source: "{#Stage}\{#UsbipInstaller}"; DestDir: "{tmp}"; Flags: deleteafterinstall; Components: client; Check: NeedsUsbipDriver
Source: "{#Stage}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Stage}\THIRD-PARTY-LICENSES.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Stage}\licenses\*"; DestDir: "{app}\licenses"; Flags: ignoreversion

[Icons]
Name: "{group}\Porthouse"; Filename: "{app}\porthouse.exe"

[Registry]
Root: HKLM; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "porthouse"; \
  ValueData: """{app}\porthouse.exe"" --minimized"; Flags: uninsdeletevalue; Tasks: autostart

[Run]
; Only SYSTEM and administrators may read the keys and the pairing PIN that the services keep here.
Filename: "{sys}\icacls.exe"; Parameters: """{commonappdata}\porthouse"" /inheritance:r /grant:r *S-1-5-18:(OI)(CI)F *S-1-5-32-544:(OI)(CI)F"; \
  Flags: runhidden waituntilterminated; StatusMsg: "Protecting the key directory..."
Filename: "{tmp}\{#UsbipInstaller}"; Parameters: "/VERYSILENT /NORESTART /SUPPRESSMSGBOXES /SP-"; \
  Flags: waituntilterminated; StatusMsg: "Installing the USB/IP driver..."; Components: client; Check: NeedsUsbipDriver
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""porthouse server (TCP)"" dir=in action=allow protocol=TCP localport=7600 profile=domain,private"; \
  Flags: runhidden waituntilterminated; Components: server
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""porthouse server (mDNS)"" dir=in action=allow protocol=UDP localport=5353 program=""{app}\porthouse-server.exe"" profile=domain,private"; \
  Flags: runhidden waituntilterminated; Components: server
Filename: "{app}\porthouse-server.exe"; Parameters: "install-service"; Flags: runhidden waituntilterminated; \
  StatusMsg: "Starting the Porthouse server service..."; Components: server
Filename: "{app}\porthouse-clientd.exe"; Parameters: "install-service"; Flags: runhidden waituntilterminated; \
  StatusMsg: "Starting the Porthouse client service..."; Components: client
Filename: "{app}\porthouse.exe"; Description: "Open porthouse"; Flags: nowait postinstall skipifsilent runasoriginaluser

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/f /im porthouse.exe"; Flags: runhidden; RunOnceId: "StopProgram"
Filename: "{app}\porthouse-server.exe"; Parameters: "uninstall-service"; Flags: runhidden; RunOnceId: "RemoveServer"; Components: server
Filename: "{app}\porthouse-clientd.exe"; Parameters: "uninstall-service"; Flags: runhidden; RunOnceId: "RemoveClient"; Components: client
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""porthouse server (TCP)"""; Flags: runhidden; RunOnceId: "RemoveRuleTcp"
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""porthouse server (mDNS)"""; Flags: runhidden; RunOnceId: "RemoveRuleMdns"

[Code]
function NeedsUsbipDriver: Boolean;
begin
  Result := not FileExists(ExpandConstant('{commonpf64}\USBip\usbip.exe'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Purge: Boolean;
begin
  if CurUninstallStep <> usPostUninstall then
    Exit;
  if UninstallSilent then
    Purge := CompareText(ExpandConstant('{param:purge|0}'), '1') = 0
  else
    Purge := MsgBox('Delete the keys and pairings porthouse saved? Keep them if you plan to install porthouse again.', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES;
  if Purge then
    DelTree(ExpandConstant('{commonappdata}\porthouse'), True, True, True);
end;
