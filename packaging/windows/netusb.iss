; Builds the netusb installer. Run through scripts/package-windows.ps1, which prepares the files.

#ifndef Version
  #error Pass the version with /DVersion=x.y.z
#endif
#ifndef Stage
  #error Pass the staging directory with /DStage=path
#endif
#define UsbipInstaller "USBip-0.9.8.1-x64.exe"

[Setup]
AppId={{4F8E91A8-94C9-47C8-AE1E-3D767B908D9B}
AppName=netusb
AppVersion={#Version}
AppPublisher=netusb
AppPublisherURL=https://github.com/RisPNG/netusb
DefaultDirName={autopf}\netusb
DefaultGroupName=netusb
DisableProgramGroupPage=yes
OutputBaseFilename=netusb-{#Version}-windows-x64-setup
OutputDir={#Stage}\..
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
WizardStyle=modern
UninstallDisplayIcon={app}\netusb.exe
SetupIconFile={#Stage}\netusb.ico
CloseApplications=yes
RestartApplications=no
LicenseFile={#Stage}\LICENSE-MIT

[Types]
Name: "full"; Description: "Use and share devices"
Name: "client"; Description: "Only use devices shared by other computers"
Name: "server"; Description: "Only share this computer's devices"
Name: "custom"; Description: "Choose what to install"; Flags: iscustom

[Components]
Name: "program"; Description: "The netusb program"; Types: full client server custom; Flags: fixed
Name: "client"; Description: "Use devices from other computers (also installs the USB/IP driver)"; Types: full client
Name: "server"; Description: "Share this computer's devices"; Types: full server

[Tasks]
Name: "autostart"; Description: "Show the netusb icon in the notification area when I sign in"

[Dirs]
Name: "{commonappdata}\netusb"

[Files]
Source: "{#Stage}\gui\*"; DestDir: "{app}"; Flags: recursesubdirs ignoreversion; Components: program
Source: "{#Stage}\netusb-client.exe"; DestDir: "{app}"; Flags: ignoreversion; Components: client
Source: "{#Stage}\netusb-clientd.exe"; DestDir: "{app}"; Flags: ignoreversion; Components: client
Source: "{#Stage}\netusb-server.exe"; DestDir: "{app}"; Flags: ignoreversion; Components: server
Source: "{#Stage}\{#UsbipInstaller}"; DestDir: "{tmp}"; Flags: deleteafterinstall; Components: client; Check: NeedsUsbipDriver
Source: "{#Stage}\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Stage}\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\netusb"; Filename: "{app}\netusb.exe"

[Registry]
Root: HKLM; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "netusb"; \
  ValueData: """{app}\netusb.exe"" --minimized"; Flags: uninsdeletevalue; Tasks: autostart

[Run]
; Only SYSTEM and administrators may read the keys and the pairing PIN that the services keep here.
Filename: "{sys}\icacls.exe"; Parameters: """{commonappdata}\netusb"" /inheritance:r /grant:r *S-1-5-18:(OI)(CI)F *S-1-5-32-544:(OI)(CI)F"; \
  Flags: runhidden waituntilterminated; StatusMsg: "Protecting the key directory..."
Filename: "{tmp}\{#UsbipInstaller}"; Parameters: "/VERYSILENT /NORESTART /SUPPRESSMSGBOXES /SP-"; \
  Flags: waituntilterminated; StatusMsg: "Installing the USB/IP driver..."; Components: client; Check: NeedsUsbipDriver
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""netusb server (TCP)"" dir=in action=allow protocol=TCP localport=7600 profile=domain,private"; \
  Flags: runhidden waituntilterminated; Components: server
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""netusb server (mDNS)"" dir=in action=allow protocol=UDP localport=5353 program=""{app}\netusb-server.exe"" profile=domain,private"; \
  Flags: runhidden waituntilterminated; Components: server
Filename: "{app}\netusb-server.exe"; Parameters: "install-service"; Flags: runhidden waituntilterminated; \
  StatusMsg: "Starting the netusb server service..."; Components: server
Filename: "{app}\netusb-clientd.exe"; Parameters: "install-service"; Flags: runhidden waituntilterminated; \
  StatusMsg: "Starting the netusb client service..."; Components: client
Filename: "{app}\netusb.exe"; Description: "Open netusb"; Flags: nowait postinstall skipifsilent runasoriginaluser

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/f /im netusb.exe"; Flags: runhidden; RunOnceId: "StopProgram"
Filename: "{app}\netusb-server.exe"; Parameters: "uninstall-service"; Flags: runhidden; RunOnceId: "RemoveServer"; Components: server
Filename: "{app}\netusb-clientd.exe"; Parameters: "uninstall-service"; Flags: runhidden; RunOnceId: "RemoveClient"; Components: client
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""netusb server (TCP)"""; Flags: runhidden; RunOnceId: "RemoveRuleTcp"
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""netusb server (mDNS)"""; Flags: runhidden; RunOnceId: "RemoveRuleMdns"

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
    Purge := MsgBox('Delete the keys and pairings netusb saved? Keep them if you plan to install netusb again.', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES;
  if Purge then
    DelTree(ExpandConstant('{commonappdata}\netusb'), True, True, True);
end;
