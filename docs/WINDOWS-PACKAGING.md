# Windows 分发包

Windows 侧有三种交付形态：**Inno 安装器**（推荐给个人用户）、**MSI**（企业批量部署）
和**便携 zip**（免安装）。每种都有 x64 与 ARM64 两个架构版本。

本文说明产物怎么来、装到哪、以及哪些地方刻意没做。

## 1. 产物

| 文件 | 说明 |
|---|---|
| `dist/windows/sadp-discover-<ver>-windows-<arch>-setup.exe` | Inno Setup 安装器：按用户安装、开始菜单快捷方式、CLI 加进 PATH、中英双语向导、自带卸载器 |
| `dist/windows/sadp-discover-<ver>-windows-<arch>.msi` | Windows Installer 包：布局与安装器一致，供 GPO / Intune / SCCM 静默部署 |
| `dist/sadp-discover-<ver>-windows-<arch>.zip` | 便携包，解开是 `sadp-discover-<ver>-windows-<arch>/` 一个目录 |
| `dist/SHA256SUMS-windows.txt` | x64 的 zip / setup / MSI 的 SHA-256 |
| `dist/SHA256SUMS-windows-arm64.txt` | ARM64 的同上一份 |

`<arch>` 是 `x86_64` 或 `arm64`。四种打包输入完全一致：`sadp-discover.exe`、
`sadp-discover-gui.exe`、`README.md`、`LICENSE-MIT`、`LICENSE-APACHE`；
公共输入目录是 `dist/windows/payload/`（x64）与 `dist/windows/payload-arm64/`（ARM64），
两个架构不能共用，否则后跑的会覆盖先跑的。

## 2. Inno 安装器

装到 **`%LOCALAPPDATA%\Programs\sadp-discover`**，`PrivilegesRequired=lowest`：
不申请管理员权限、不弹 UAC。代价是不能装进 `Program Files`，也没有「为所有用户安装」的
选项——这个是刻意的取舍，见 §7。

| 动作 | 落在哪 |
|---|---|
| 文件 | `{app}` = `%LOCALAPPDATA%\Programs\sadp-discover` |
| 快捷方式 | 当前用户的开始菜单 `sadp-discover.lnk`；桌面图标是可选任务（默认不勾） |
| PATH | `HKCU\Environment\Path` 末尾追加 `{app}`，`REG_EXPAND_SZ` |
| 卸载信息 | `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\...`，含版本号与图标 |
| 卸载器 | `{app}\unins000.exe` |

PATH 的增删写在 `[Code]` 里而不是 `[Registry]`：`[Registry]` 在卸载时只会删掉
「整条值」，而 `Path` 是用户原有的、用分号拼起来的一串，必须拆开、只摘掉我们那一节。
`ChangesEnvironment=yes` 让资源管理器在装完/卸完后刷新环境变量，但**已经打开**的
命令行窗口不会跟着变，得新开一个。

安装器带向导：欢迎 → 许可（MIT）→ 安装前说明（安装位置/首次运行/卸载）→ 选择目录 →
任务（PATH、桌面图标）→ 安装 → 完成。

### 2.1 中文向导

`[Languages]` 挂了两份：英文（`compiler:Default.isl`）与简体中文
（`packaging/windows/installer/languages/ChineseSimplified.isl`）。`ShowLanguageDialog=auto`
在系统语言匹配时自动选，否则弹语言选择。

**为什么语言文件随仓库走**：Inno 6.7 的安装目录里没有 `ChineseSimplified.isl`
（简体中文 7.x 才进主干），引用 `compiler:Languages\...` 在 CI 上必然编译失败。
仓库里这份取自官方源码仓库 `issrc` 的 `is-6_7_4` 标签，与 6.7 的消息集对齐；
升级 Inno 大版本时按 `languages/README.md` 重新取一份，少消息会在编译期报错。

自定义文案（PATH、环境变量两项任务）在 `[CustomMessages]` 里中英各一份；
漏翻会直接显示成英文，但不会编译失败。

### 2.2 架构

- x64：`ArchitecturesAllowed=x64compatible`，ARM64 Windows 上也能以仿真方式安装。
- ARM64：`/DArchAllowed=arm64`，在 x64 上安装器会拒绝运行。

`scripts/windows/installer.ps1` 把 `ArchAllowed` 与 payload 目录名作为 `/D` 参数传给 ISCC。

## 3. MSI

`packaging/windows/msi/product.wxs` + WiX Toolset 3.14（`candle` / `light`）。

定位是**企业静默部署**：没有向导、没有许可页，`msiexec /qn` 装完即用。安装布局与 Inno
安装器完全一致：`%LOCALAPPDATA%\Programs\sadp-discover`、HKCU 的 PATH、当前用户开始菜单。

| 组件 | 内容 | KeyPath |
|---|---|---|
| `MainBinaries` | 两个 exe + README + 两份许可 | HKCU `Software\sadp-discover\Binaries` |
| `PathEnvironment` | `Environment` 表把 `[INSTALLFOLDER]` 追加进用户 PATH | HKCU `Software\sadp-discover\PathEntry` |
| `StartMenuShortcut` | 开始菜单根下的 `sadp-discover.lnk` | HKCU `Software\sadp-discover\StartMenuShortcut` |

几个踩过的坑，改 wxs 时别退回去：

- **按用户安装的组件不能用文件当 KeyPath**（ICE38），必须挂 HKCU 注册表值；
  用户目录下的文件夹要写 `RemoveFolder`（ICE64），否则卸载后留空目录。
- **`InstallScope="perUser"`**：不写这个，MSI 默认按机器装，会去写 `Program Files` 与 HKLM。
- **`Codepage="936"`**：`DowngradeErrorMessage` 是中文，默认 1252 会让 light 直接报 LGHT0311。
- **`MajorUpgrade AllowSameVersionUpgrades="yes"`**：CI 冒烟会反复装同一个版本；
  代价是 light 会报 ICE61 警告（无害）。
- **PATH 值带尾反斜杠**：MSI 的 `Environment` 表把目录属性格式化成 `C:\...\sadp-discover\`，
  Inno 不带。脚本与冒烟比较前统一 `TrimEnd('\')`。
- **不要用 `INSTALLFOLDER=` 覆盖安装目录做冒烟**：覆盖值不会在卸载时保留，卸载会去删
  默认目录，自定义目录与 PATH 全部残留。冒烟固定用默认目录。

```powershell
msiexec /i .\sadp-discover-<ver>-windows-x86_64.msi /qn /norestart /l*v install.log
msiexec /x .\sadp-discover-<ver>-windows-x86_64.msi /qn /norestart
```

与 Inno 安装器**二选一**：两者写同一个目录、同一条 PATH、同一个快捷方式，混装后卸载
其中一个会把另一个的入口拆掉。

## 4. exe 上的资源

两个 exe 都带图标、应用清单与版本信息，来源是 `packaging/windows/app.rc.in`：

- `1 ICON`：资源管理器、任务栏、快捷方式用的图标，由 `packaging/make-icon.py` 从
  同一张 1024pt 母版生成（`assets/sadp-discover.ico`）；
- `1 24`（`RT_MANIFEST`）：`packaging/windows/app.manifest`，声明 `asInvoker`
  （不申请管理员）、PerMonitorV2 DPI 感知、`longPathAware`、兼容 Win10/11；
- `1 VERSIONINFO`：文件属性「详细信息」里的产品名、版本、版权。

模板里的 `@占位符@` 由 `crates/sadp-cli/build.rs` 与 `crates/sadp-gui/build.rs`
填好（共用 `packaging/windows/build-resources.rs`），写进 `OUT_DIR` 再交给
`rc.exe` 编译。生成的 `.rc` 在 `OUT_DIR` 里，所以图标与清单一律换成绝对路径。

目标平台不是 Windows 时 build.rs **直接返回**：本地 macOS/Linux 构建、
`cargo check --target x86_64-pc-windows-msvc` 的交叉检查都不需要 `rc.exe`。
主机不是 Windows 但目标是时（交叉编译）会打一条警告并跳过——资源由 CI 的
Windows runner 生产。

## 5. 控制台

GUI 用 `#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]`
发布：双击不再附送一个空的黑色控制台窗口。debug 构建保持控制台，`cargo run` 照旧看得到日志。

代价是进程启动时没有标准输出，`--version`/`--help` 会没地方写。所以 `main` 的第一件事是
`console::attach_parent_console()`（`crates/sadp-gui/src/console.rs`）：

1. 先看 `GetStdHandle(STD_OUTPUT_HANDLE)` 的文件类型：磁盘/管道/字符设备说明输出
   已经被重定向（`> out.txt`、`| more`、或本来就是控制台），尊重它，什么都不做；
2. 句柄无效（`FILE_TYPE_UNKNOWN`，Explorer 双击的典型情况）才
   `AttachConsole(ATTACH_PARENT_PROCESS)`，失败说明父进程没有控制台，静默返回；
3. 挂上了就 `CreateFileW("CONOUT$")` 再 `SetStdHandle` 到 stdout/stderr。

句柄不关闭：标准句柄要活到进程结束。

## 6. 脚本

| 脚本 | 职责 |
|---|---|
| `scripts/build-windows.ps1` | 编排入口：build → package → installer → msi → 校验和；`-Arch`、`-Skip*` |
| `scripts/windows/config.ps1` | 单一事实来源：版本号、架构、产物路径、PE 头断言、WiX 定位 |
| `scripts/windows/build.ps1` | `cargo build --release --locked`，版本探针、资源检查、PE 架构断言 |
| `scripts/windows/package.ps1` | 整理 `payload(-arm64)`，打 zip，校验 zip 顶层只有一个版本目录 |
| `scripts/windows/installer.ps1` | 找 `ISCC.exe`，传版本/架构/payload 定义，编译安装器 |
| `scripts/windows/msi.ps1` | 找 WiX，`candle` + `light` 出 MSI |
| `scripts/windows/msvc-env.ps1` | ARM64 交叉编译的 PATH/LIB/INCLUDE（不依赖 vcvarsall.bat） |
| `scripts/windows/smoke-install.ps1` | Inno 静默装到临时目录 → 跑两个 exe → 查快捷方式/PATH → 静默卸载 → 查残留 |
| `scripts/windows/smoke-msi.ps1` | 同上，走 `msiexec`，固定装默认目录 |

版本号由 `cargo pkgid -p sadp-cli` 取得，与 `release` 的版本闸门同源。
预发布 tag（`v0.1.0-rc.1`）会保留在文件名里，但 `VersionInfoVersion` 只取前三位数字。

## 7. 刻意没做的

- **Authenticode 签名**：没有代码签名证书，SmartScreen 会对未签名的安装器与 MSI
  报「未知发布者」。签名需要 `signtool` + 证书（或 Azure Trusted Signing），
  加进来就是一段没人验证过的代码，所以宁可空着——和 macOS 侧对待签名密钥的态度一致。
  证书到位后的补法见 §8。
- **x86（32 位）**：只出 x64 与 ARM64。x86 Windows 已不在支持范围；ARM64 Windows
  能用 x64 仿真跑 x64 包。
- **MSI 的图形界面 / 许可页**：企业部署用 `/qn`，个人用户用 Inno 安装器；给 MSI 加
  `WixUI_*` 只会多一套没人走的 UI 与一份 RTF 许可。
- **防火墙规则**：不做自动加规则——那需要管理员权限，与「按用户安装」冲突。
  首次运行时由系统弹窗询问，说明页里也写了手动补规则的命令。

## 8. 未签名安装器的用户预期

没有 Authenticode 证书时，`setup.exe` / `.msi` 首次运行会弹 SmartScreen「Windows 已保护
你的电脑」：点「更多信息」→「仍要运行」。从 zip 解压的 exe 可能带 Mark-of-the-Web，
可先 `Unblock-File .\sadp-discover-gui.exe`，之后再运行就不会再弹「打开文件 - 安全警告」。

校验下载完整性：

```powershell
Get-FileHash -Algorithm SHA256 .\sadp-discover-<ver>-windows-x86_64-setup.exe
Get-FileHash -Algorithm SHA256 .\sadp-discover-<ver>-windows-x86_64.zip
Get-FileHash -Algorithm SHA256 .\sadp-discover-<ver>-windows-x86_64.msi
# 与 dist/SHA256SUMS-windows.txt（Release 里同名文件）逐行对照
```

这是**刻意的现状**，不是漏了一步：证书到位后，在 `installer.ps1` 编译完安装器、以及
`msi.ps1` 的 `light` 之后，各加一段
`signtool sign /fd SHA256 /tr <RFC3161 时间戳> /td SHA256` 即可（zip 里的两个 exe 也要签）；
产物名、安装布局、PATH 行为都不变。届时 `Get-AuthenticodeSignature` 应返回
`Status = Valid`，SmartScreen 的「未知发布者」才会消失。

## 9. ARM64

在 x64 Windows 上交叉编译：

```powershell
pwsh -File scripts/build-windows.ps1 -Arch arm64
```

前置条件：Visual Studio 的 **ARM64 C++ 工具链**
（`Microsoft.VisualStudio.Component.VC.Tools.ARM64`）。`scripts/windows/msvc-env.ps1`
不调用 `vcvarsall.bat`（某些受限会话里 `cmd.exe` 直接返回 `ERROR_ELEVATION_REQUIRED`），
而是自己拼 PATH / LIB / INCLUDE：`link.exe` 用 `Hostx64\x64` 的（链接器本身跨架构），
库指向 MSVC `lib\arm64` 与 Windows SDK 的 `ucrt/um\arm64`。

本地没有这个组件时，可以用 [xwin](https://github.com/Jake-Shadle/xwin) 下载 CRT/SDK：

```powershell
xwin --accept-license splat --output D:\tools\xwin-splat --preserve-ms-arch-notation --disable-symlinks
$env:SADP_XWIN_SPLAT = 'D:\tools\xwin-splat'
pwsh -File scripts/build-windows.ps1 -Arch arm64
```

（xwin 需要能创建符号链接；没有权限时加 `--disable-symlinks`，某些版本仍会尝试建
版本目录链接，失败就只能在装了 VS ARM64 组件的机器或 CI 上构建。）

防呆：`build.ps1` 读 PE 头的 Machine 字段断言 0xAA64，x64 主机不执行 ARM64 的
`--version` 探针（执行会得到「无法启动此程序」这种与根因无关的报错）。
安装器与 MSI 都按架构生成，文件名带 `-windows-arm64`；安装器在 x64 上会拒绝安装。

## 10. CI 里验证了什么

`dev` 工作流在 push 到 `main` 时跑两个打包 job（PR 不跑，省 runner 时间）：

| job | 做什么 |
|---|---|
| `package (windows)` | x64 全链路：build → zip → Inno → MSI → 校验和 → **装/卸 Inno** → **装/卸 MSI** |
| `package (windows arm64)` | 在 x64 runner 上交叉编译 ARM64 → zip → Inno → MSI；不跑安装冒烟（架构不同），由 PE 头断言兜底 |

`release` 在打 tag 时在同一个 job 里依次构建 x64 与 ARM64，x64 跑两套安装冒烟，
最后把六件产物（两架构 × zip/setup/MSI）与校验和一起上传。

| 检查 | 手段 |
|---|---|
| 二进制能启动、`--version` 有输出 | `build.ps1` 用 `Start-Process` 重定向 stdout 再比对退出码——GUI 是子系统进程，`&` 拿不到它的输出 |
| 资源真的嵌进去了 | 读 `VersionInfo.FileVersion`；为空说明 rc.exe 没编进去（图标/清单/版本会一起缺失） |
| 交叉编译没有「假成功」 | 读 PE 头 `Machine`，x64 = 0x8664、ARM64 = 0xAA64 |
| 安装器能装 | `/VERYSILENT /SUPPRESSMSGBOXES /DIR=<临时目录>`，检查两个 exe、开始菜单快捷方式、HKCU 的 PATH |
| MSI 能装 | `msiexec /i ... /qn`，同样检查文件/快捷方式/PATH，并确认卸载注册表项 |
| 装完能跑 | 对安装目录里的两个 exe 再跑一次 `--version` |
| 能卸干净 | Inno 跑 `unins000.exe /VERYSILENT`；MSI 用 `msiexec /x`；两者都轮询等文件消失，再确认 PATH 与卸载项没有残留 |
| 中文向导能编译 | ISCC 在 CI 上解析随仓库携带的 `.isl`；少消息会直接编译失败 |

`dev` 的打包 job 把失败日志的尾部塞进 GitHub annotation（`::error`）：
仓库没有 gh token，runner 的日志下载不了，annotation 是公开仓库里唯一能匿名读到
错误原文的通道。
