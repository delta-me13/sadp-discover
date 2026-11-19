# Windows 分发包

Windows 侧产出两个形态：**安装器**（推荐）与**便携 zip**。两者装的是同一批文件，
只是交付方式不同。本文说明产物怎么来、装到哪、以及哪些地方刻意没做。

## 1. 产物

| 文件 | 说明 |
|---|---|
| `dist/windows/sadp-discover-<ver>-windows-x86_64-setup.exe` | Inno Setup 安装器：按用户安装、开始菜单快捷方式、CLI 加进 PATH、自带卸载器 |
| `dist/sadp-discover-<ver>-windows-x86_64.zip` | 便携包，解开是`sadp-discover-<ver>-windows-x86_64/` 一个目录 |
| `dist/SHA256SUMS-windows.txt` | 上面两个文件的 SHA-256 |

安装器与 zip 的内容完全一致：`sadp-discover.exe`、`sadp-discover-gui.exe`、
`README.md`、`LICENSE-MIT`、`LICENSE-APACHE`。公共输入是
`dist/windows/payload/`——安装器从这里取文件，zip 从这里打包（外面多包一层目录，
免得用户解压到下载目录时把 exe 撒一地）。

## 2. 安装器

装到 **`%LOCALAPPDATA%\Programs\sadp-discover`**，`PrivilegesRequired=lowest`：
不申请管理员权限、不弹 UAC。代价是不能装进 `Program Files`，也没有「为所有用户安装」
的选项——这个是刻意的取舍，见 §6。

| 动作 | 落在哪 |
|---|---|
| 文件 | `{app}` = `%LOCALAPPDATA%\Programs\sadp-discover` |
| 快捷方式 | 当前用户的开始菜单`sadp-discover.lnk`；桌面图标是可选任务（默认不勾） |
| PATH | `HKCU\Environment\Path` 末尾追加 `{app}`，`REG_EXPAND_SZ` |
| 卸载信息 | `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\...`，含版本号与图标 |
| 卸载器 | `{app}\unins000.exe` |

PATH 的增删写在 `[Code]` 里而不是 `[Registry]`：`[Registry]` 在卸载时只会删掉
「整条值」，而 `Path` 是用户原有的、用分号拼起来的一串，必须拆开、只摘掉我们那一节。
`ChangesEnvironment=yes` 让资源管理器在装完/卸完后刷新环境变量，但**已经打开**的
命令行窗口不会跟着变，得新开一个。

安装器带向导：欢迎 → 许可（MIT）→ 安装前说明（安装位置/首次运行/卸载）→ 选择目录 →
任务（PATH、桌面图标）→ 安装 → 完成。

**向导界面目前只有英文**：Inno Setup 6.7 的 `Languages\` 目录里没有
`ChineseSimplified.isl`（简体中文是社区翻译，7.0 之后才并进主干），拿不到就只能退回
`Default.isl`。装前说明页、项目 README、程序界面都是中文，缺的只是「下一步/安装」
这类按钮文案。等 runner 的 Inno 升到 7.x，或者仓库里自带一份 .isl，就能切回中文。

## 3. exe 上的资源

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

## 4. 控制台

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

## 5. 脚本

| 脚本 | 职责 |
|---|---|
| `scripts/build-windows.ps1` | 编排入口：build → package → installer → 校验和 |
| `scripts/windows/config.ps1` | 单一事实来源：版本号、产物路径、文件名；被其它脚本点号引入 |
| `scripts/windows/build.ps1` | `cargo build --release --locked`，跑 `--version` 探针，检查资源是否真的嵌进去了 |
| `scripts/windows/package.ps1` | 整理 `payload/`，打 zip，并校验 zip 顶层只有一个版本目录 |
| `scripts/windows/installer.ps1` | 找 `ISCC.exe`，把版本号作为 `/D` 传给 Inno Setup |
| `scripts/windows/smoke-install.ps1` | 静默装到临时目录 → 跑两个 exe → 查快捷方式与 PATH → 静默卸载 → 查残留 |

版本号由 `cargo pkgid -p sadp-cli` 取得，与 `release` 的版本闸门同源。
预发布 tag（`v0.1.0-rc.1`）会保留在文件名里，但 `VersionInfoVersion` 只取前三位数字。

本地需要 Inno Setup 6（`ISCC.exe`）；找不到时 `installer.ps1` 会给出提示，
也可以用 `-Iscc` 或 `ISCC` 环境变量指定路径。GitHub 的 `windows-latest`
镜像预装了 Inno Setup 6.7 与 WiX Toolset 3.14。

## 6. 刻意没做的

- **Authenticode 签名**：没有代码签名证书，SmartScreen 会对未签名的安装器
  报「未知发布者」。签名需要 `signtool` + 证书（或 Azure Trusted Signing），
  加进来就是一段没人验证过的代码，所以宁可空着——和 macOS 侧对待签名密钥的态度一致。
- **MSI**：企业批量部署更认 MSI（GPO/Intune）。Inno Setup 的 `.exe` 能用
  `/VERYSILENT /SUPPRESSMSGBOXES` 静默安装，够用；真要做 MSI 再补 WiX 源。
- **x86 / ARM64**：只出 `x86_64`。ARM64 Windows 能通过 x64 仿真跑，没做原生包。
- **防火墙规则**：不做自动加规则——那需要管理员权限，与「按用户安装」冲突。
  首次运行时由系统弹窗询问，说明页里也写了手动补规则的命令。

### 6.1 未签名安装器的用户预期

没有 Authenticode 证书时，`setup.exe` 首次运行会弹 SmartScreen「Windows 已保护你的电脑」：
点「更多信息」→「仍要运行」。从 zip 解压的 exe 可能带 Mark-of-the-Web，可先
`Unblock-File .\sadp-discover-gui.exe`，之后再运行就不会再弹「打开文件 - 安全警告」。

校验下载完整性：

```powershell
Get-FileHash -Algorithm SHA256 .\sadp-discover-<ver>-windows-x86_64-setup.exe
Get-FileHash -Algorithm SHA256 .\sadp-discover-<ver>-windows-x86_64.zip
# 与 dist/SHA256SUMS-windows.txt（Release 里同名文件）逐行对照
```

这是**刻意的现状**，不是漏了一步：证书到位后，在 `installer.ps1` 编译完安装器之后加一段
`signtool sign /fd SHA256 /tr <RFC3161 时间戳> /td SHA256`，再对 zip 里的两个 exe 各签一次
即可；产物名、安装布局、PATH 行为都不变。届时 `Get-AuthenticodeSignature` 应返回
`Status = Valid`，SmartScreen 的「未知发布者」才会消失。

## 7. CI 里验证了什么

`dev` 工作流在 push 到 `main` 时跑 `package (windows)`（PR 不跑，省 runner 时间），
`release` 在打 tag 时跑同一套脚本。两条都会做：

| 检查 | 手段 |
|---|---|
| 二进制能启动、`--version` 有输出 | `build.ps1` 用 `Start-Process` 重定向 stdout 再比对退出码——GUI 是子系统进程，`&` 拿不到它的输出 |
| 资源真的嵌进去了 | 读 `VersionInfo.FileVersion`；为空说明 rc.exe 没编进去（图标/清单/版本会一起缺失） |
| 安装器能装 | `/VERYSILENT /SUPPRESSMSGBOXES /DIR=<临时目录>`，检查两个 exe、开始菜单快捷方式、HKCU 的 PATH |
| 装完能跑 | 对安装目录里的两个 exe 再跑一次 `--version` |
| 能卸干净 | 跑 `unins000.exe /VERYSILENT`，轮询等文件消失，再确认 PATH 里没有残留 |

首次跑通时 `package (windows)` 一共 367 秒，其中 301 秒是全量 LTO 的冷编译，
冒烟本身只占 5 秒。

`dev` 的这条 job 把失败日志的尾部塞进 GitHub annotation（`::error`）：
仓库没有 gh token，runner 的日志下载不了，annotation 是公开仓库里唯一能匿名读到
错误原文的通道。
