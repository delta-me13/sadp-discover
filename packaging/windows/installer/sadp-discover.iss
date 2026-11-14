; sadp-discover 的 Windows 安装器（Inno Setup 6，6.7+ 自带简体中文语言文件）。
;
; 只做三件事：把两个 exe 放进用户目录、给 GUI 建快捷方式、把 CLI 加进 PATH。
; 装到 %LOCALAPPDATA%\Programs\sadp-discover，不需要管理员权限；
; 卸载走「设置 → 应用 → 已安装的应用」或安装目录下的 unins000.exe。
;
; 命令行只传两个不含空格的定义（版本号与产物名），其余路径都由 ISPP 从本文件
; 所在目录算出来——这样仓库路径里有空格也不会被拆开。

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef AppFileVersion
  #define AppFileVersion "0.0.0.0"
#endif
#ifndef OutputBaseFilename
  #define OutputBaseFilename "sadp-discover-setup"
#endif

#define AppName "sadp-discover"
#define AppPublisher "delta-me13"
#define AppUrl "https://github.com/delta-me13/sadp-discover"
#define AppExe "sadp-discover-gui.exe"

; 脚本在 packaging/windows/installer/，往上三层是仓库根。
#define RepoRoot SourcePath + "\..\..\.."

[Setup]
; AppId 一旦发布就不能改：Inno 靠它识别「这是同一个软件的升级」。
AppId={{B0C4F1E2-8A37-4D6B-9E85-3C7A1F2D6B94}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}/issues
AppUpdatesURL={#AppUrl}/releases
VersionInfoVersion={#AppFileVersion}
VersionInfoCompany={#AppPublisher}
VersionInfoDescription=sadp-discover setup
VersionInfoCopyright=sadp-discover contributors, MIT OR Apache-2.0

; 按用户安装：不弹 UAC，装完就能用，卸载干净。
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
MinVersion=10.0
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

OutputDir={#RepoRoot}\dist\windows
OutputBaseFilename={#OutputBaseFilename}
SourceDir={#RepoRoot}\dist\windows\payload
SetupIconFile={#RepoRoot}\assets\sadp-discover.ico
UninstallDisplayIcon={app}\{#AppExe}

; 许可页直接用仓库根的双许可说明里的 MIT 分支；Apache 全文随包装进 {app}。
LicenseFile={#RepoRoot}\LICENSE-MIT
InfoBeforeFile={#RepoRoot}\packaging\windows\installer\readme.txt

Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; 改过 PATH，装完/卸完都要通知资源管理器刷新环境变量。
ChangesEnvironment=yes
ShowLanguageDialog=auto

[Languages]
Name: "chinesesimplified"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
AddToPath=Add sadp-discover to PATH
AddToPath.chinesesimplified=把 sadp-discover 加入 PATH（方便在命令行里直接用）
ModifyEnvironment=Environment
ModifyEnvironment.chinesesimplified=环境变量

[Tasks]
Name: "addtopath"; Description: "{cm:AddToPath}"; GroupDescription: "{cm:ModifyEnvironment}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "sadp-discover.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "sadp-discover-gui.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[Code]
// PATH 的增删只能写在代码里：Inno 的 [Registry] 表在卸载时是「删除自己写的那条值」，
// 而 PATH 是用户原有的、用分号拼起来的一串，必须拆开、只摘掉我们那一节。

const
  EnvKey = 'Environment';

function PathHas(Paths: String; Dir: String): Boolean;
begin
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Paths) + ';') > 0;
end;

procedure AddToPath(const Dir: String);
var
  Paths: String;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Paths) then
    Paths := '';
  if PathHas(Paths, Dir) then
    Exit;
  if Paths = '' then
    Paths := Dir
  else
    Paths := Paths + ';' + Dir;
  RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Paths);
end;

function RemoveFromPath(const Dir: String): Boolean;
var
  Paths, Rest, Item, Kept: String;
  P: Integer;
begin
  Result := False;
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Paths) then
    Exit;

  Rest := Paths;
  Kept := '';
  while Rest <> '' do
  begin
    P := Pos(';', Rest);
    if P = 0 then
    begin
      Item := Rest;
      Rest := '';
    end
    else
    begin
      Item := Copy(Rest, 1, P - 1);
      Rest := Copy(Rest, P + 1, Length(Rest));
    end;

    if Item <> '' then
    begin
      if Uppercase(Item) = Uppercase(Dir) then
        Result := True
      else if Kept = '' then
        Kept := Item
      else
        Kept := Kept + ';' + Item;
    end;
  end;

  if Result then
    RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvKey, 'Path', Kept);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addtopath') then
    AddToPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveFromPath(ExpandConstant('{app}'));
end;
