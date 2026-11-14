# macOS 打包方案（.pkg + .app + PATH 里的 CLI）

> 目标：**一个 .pkg 同时交付两件事**——点击 .app 启动 GUI；终端里任何目录都能直接跑 `sadp-discover`。
> 本文只讲 macOS 分发形态，协议/扫描行为见 [MACOS.md](./MACOS.md)。
>
> **状态**：M1/M2 已实现并在 macOS 26 / Apple Silicon 上跑通（ad-hoc 签名，未公证）。
> app 内 CLI 与 GUI 均为 universal（x86_64 + arm64，两个 slice 的 minos 都是 11.0），
> Intel slice 已用 Rosetta 实际运行验证过。
> 实现位于 `scripts/macos/bundle.sh`（组装 + 签名）、`scripts/macos/pkg.sh`（打 pkg）、
> `scripts/macos/dmg.sh`（打 dmg）、`packaging/macos/Info.plist`；
> 图标由 `packaging/macos/make-icon.py` 生成到 `assets/`。
> M4 的 dmg 与 SHA256SUMS 已实现并接进 release 流水线；pkg 是带向导的 product archive
> （`packaging/macos/installer/`），授权文案有中英两份 `InfoPlist.strings`，
> GUI 的致命错误在双击启动时会弹窗。
> 真机验收 8 条里依赖证书的 6/7 尚未执行；签名/公证（M3）需要证书，尚未执行。

## 0. 验收标准

包做完后用下面 8 条判定，缺一条都算没做完：

| # | 场景 | 期望 |
|---|---|---|
| 1 | Finder 双击 `sadp-discover.app` | 出窗口、有 Dock 图标、菜单栏显示 sadp-discover，无终端窗口 |
| 2 | 新开终端 `sadp-discover --version` | 直接可用，不需要 `cd` 或写全路径 |
| 3 | `which sadp-discover` | `/usr/local/bin/sadp-discover` |
| 4 | `sadp-discover`（TUI） | 与 .app 同版本、同一份协议实现，行为一致 |
| 5 | 首次联网扫描 | 弹出「本地网络」授权，文案可读（来自 Info.plist） |
| 6 | `spctl -a -vvv -t exec` / `-t install` | `accepted, source=Notarized Developer ID` |
| 7 | 断网后首次启动 | 仍能启动（已 staple，不依赖在线查票） |
| 8 | 卸载 | 三条 rm 之后无残留（见 §9） |

## 1. 产物形态

| 产物 | 内容 | 定位 |
|---|---|---|
| `sadp-discover-<ver>-macos.pkg` | `/Applications/sadp-discover.app` + `/usr/local/bin/sadp-discover` | **主产物**，唯一能同时满足两个需求的形态 |
| `sadp-discover-<ver>-macos.dmg` | .app + `Applications` 软链 | 只想要 GUI 的用户（已实现，见 §5.6） |
| `sadp-discover-<ver>-macos-universal.tar.gz` | 裸 CLI | 保留现状：Homebrew / 脚本 / 容器 |

为什么主产物是 pkg 而不是 dmg：**dmg 无法安全地写 `/usr/local/bin`**。要么让用户手动 `mv`（PATH 需求落不实），要么在 dmg 里塞一个「双击运行我」的脚本（用户基本不点，且被 Gatekeeper 拦）。pkg 是唯一「一次点击，两件事都到位」的载体。

为什么不是 Homebrew cask：cask 只装 .app，不 link CLI；要让 CLI 进 PATH 得再维护一个 formula 或 tap 仓库，属于**补充渠道**（§8），不是首发方案。

## 2. .app 里到底放什么

```
sadp-discover.app/
└── Contents/
    ├── Info.plist                      # 见 §3
    ├── PkgInfo                         # 一行：APPL????
    ├── MacOS/
    │   ├── sadp-discover-gui           # CFBundleExecutable，universal
    │   └── sadp-discover               # CLI（随包同版本），universal
    ├── Resources/
    │   ├── sadp-discover.icns          # packaging/macos/make-icon.py 生成
    │   ├── zh-Hans.lproj/
    │   │   └── InfoPlist.strings       # 本地化 NSLocalNetworkUsageDescription
    │   └── en.lproj/
    │       └── InfoPlist.strings       # 缺了它，英文系统会看到中文授权弹窗
    └── _CodeSignature/                 # codesign 生成，不要手工创建
```

CLI 放 `Contents/MacOS/` 而不是 `Contents/Resources/bin/`：可执行位、`codesign` 的嵌套代码枚举、`spctl` 的判定都按 `MacOS/` 走；放 Resources 里虽然能跑，但属于「非标准位置的可执行代码」，公证时容易被挑刺。

### 不需要包含的东西（别多塞）

| 东西 | 为什么不需要 |
|---|---|
| `locales/*.yml` | `rust-i18n` 的 `i18n!` 宏**编译期内联**，YAML 不进二进制（`crates/sadp-core/build.rs` 就是为这个建的） |
| 任何 `.dylib` / 运行时 | 纯 Rust + 系统框架（Metal/AppKit），无第三方动态库 |
| 中文字体 | `fonts.rs` 用 fontdb 查系统字体，首选 PingFang SC |
| 配置文件 | 配置全部来自 CLI 参数，日志/数据走 `~/Library/Application Support/sadp-discover` |
| Python / Node / ffmpeg | 无外部依赖 |
| .DS_Store、`._*` | 会让公证报 `Invalid extended attribute`，打包前必须清掉 |

去掉这些之后，app 的体积基本就是两个 strip 过的二进制之和（CLI 约 2–4 MB，GUI 约 10–15 MB，universal 后合计约 25–35 MB）。

## 3. Info.plist

必填项与建议值（`<ver>` 取 workspace 的 `version`，CI 里用 tag 覆盖）：

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>                  <string>sadp-discover</string>
  <key>CFBundleDisplayName</key>           <string>sadp-discover</string>
  <key>CFBundleIdentifier</key>            <string>io.github.delta-me13.sadp-discover</string>
  <key>CFBundleExecutable</key>            <string>sadp-discover-gui</string>
  <key>CFBundlePackageType</key>           <string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key> <string>6.0</string>
  <key>CFBundleShortVersionString</key>    <string><ver></string>
  <key>CFBundleVersion</key>               <string><build></string>
  <key>CFBundleIconFile</key>              <string>sadp-discover.icns</string>
  <key>CFBundleDevelopmentRegion</key>     <string>zh-Hans</string>
  <key>CFBundleLocalizations</key>         <array><string>zh-Hans</string><string>en</string></array>
  <key>LSMinimumSystemVersion</key>        <string>11.0</string>
  <key>LSApplicationCategoryType</key>     <string>public.app-category.utilities</string>
  <key>NSHighResolutionCapable</key>       <true/>
  <key>NSHumanReadableCopyright</key>      <string>MIT OR Apache-2.0</string>
  <key>NSLocalNetworkUsageDescription</key>
  <string>用于在局域网内发现海康 SADP 设备（UDP 37020 广播与组播）。</string>
</dict>
</plist>
```

几条硬约束：

- **`CFBundleIdentifier` 一旦发版就别改**：macOS 15+ 的本地网络授权（TCC）按 bundle id 记账，改一次等于让所有老用户重新授权。
- **`CFBundleExecutable` 必须叫 `sadp-discover-gui`**（crate 的 bin 名），写错的表现是双击闪一下就退，控制台只有 `LaunchServices: could not find executable`。
- `NSLocalNetworkUsageDescription` 只对 **.app 生效**。CLI 从终端跑时，TCC 把权限记在终端 App 上，与本文无关（见 MACOS.md §3）。
- `LSMinimumSystemVersion` 要与实际链接的 deployment target 一致，见 §6 的 `MACOSX_DEPLOYMENT_TARGET`。**不要**写得比实际低，否则在旧系统上会以 `Symbol not found` 崩，而不是给出「系统版本过低」的提示。
- 菜单栏里 App 菜单显示的是 `CFBundleName`，不是二进制名，所以这里必须写 `sadp-discover`。

### 本地化（InfoPlist.strings）

能本地化的只有「面向用户的字符串」，实现方式是 `Resources/<lang>.lproj/InfoPlist.strings`：

```
"NSLocalNetworkUsageDescription" = "用于在局域网内发现海康 SADP 设备：…";
```

- 文件名固定，语言由 `.lproj` 目录名决定（`zh-Hans`、`en`），系统按用户偏好挑一个；
- 没匹配到的语言**静默回落到 Info.plist 里的值**，所以 `en` 系统要单独给一份，否则看到的是中文；
- `CFBundleDevelopmentRegion` 要与其中一种一致（现在是 `zh-Hans`），`CFBundleLocalizations` 负责声明支持范围；
- `.strings` 是纯文本，写错了不会报错、只会静默回落——`bundle.sh` 里用 `plutil -lint` 挡了一道。

不改名 `CFBundleDisplayName` 是故意的：Finder 里的名字与窗口标题、二进制名保持一致，避免「三处三个名字」。

## 4. CLI 进 PATH：两条路，推荐第二条

### 方案 A：`/usr/local/bin/sadp-discover` → 软链进 .app

- ✅ 只有一份二进制，版本天然同步，`codesign -v` 能顺着链接验证
- ❌ 用户从「访达」把 app 拖进废纸篓后，CLI 立刻变成 `No such file or directory`（报错还很难懂）
- ❌ 需要在 pkg 里塞 postinstall 脚本建链接（多一段 root 逻辑要维护）

### 方案 B：`/usr/local/bin/sadp-discover` 独立一份（推荐）

- ✅ **pkg payload 直接摆文件，不需要 postinstall 脚本**：没有脚本就没有「安装脚本执行失败」这类故障面，`pkgutil --payload-files` 还能把装了什么列全
- ✅ 删掉 .app 不影响 CLI；CLI 是这个项目的主界面（TUI/脚本/CI），不该是 GUI 的附属品
- ✅ 两份二进制**逐字节相同**（同一次 lipo 产物复制两次），CDHash 一致，所以 .app 的公证票据同样覆盖 `/usr/local/bin` 那份
- ✅ pkg 一次原子更新两者，不存在版本漂移
- ❌ 磁盘上多一份（约 10 MB，可接受）

`/usr/local/bin` 在 macOS 默认 `PATH` 里（`/etc/paths` 含它），Apple Silicon 装 Homebrew 也不会把它挤掉。pkg 以 root 安装，写这里不需要额外授权。

Homebrew 用户可能已有同名文件：pkg 会直接覆盖，属于预期行为，在 release notes 里写一句「Homebrew 用户请走 tap，不要混装」。

## 5. 构建流水线

### 5.0 前置条件

只需要 **Xcode Command Line Tools**，不需要完整 Xcode：`codesign`、`pkgbuild`、`productsign`、`stapler`、`notarytool`、`hdiutil`、`iconutil`、`vtool` 都在 CLT 里（已在 macOS 26 + CLT 上逐个确认）。

签名/公证另需：

- `security find-identity -v -p codesigning` 里至少有一张 **Developer ID Application** 证书；
- 打 pkg 另需 **Developer ID Installer** 证书（`productsign` 用）；
- App Store Connect API Key（`.p8` + Key ID + Issuer ID），CI 用它替代账号密码。

这些都是**机器上的状态**，不是仓库里的文件——本地没有证书时构建脚本必须能降级到 ad-hoc 签名并继续走完组装/安装流程。

### 5.1 构建与合成

```bash
export MACOSX_DEPLOYMENT_TARGET=11.0          # 两个 slice 必须一致
for t in aarch64-apple-darwin x86_64-apple-darwin; do
  cargo build --release -p sadp-cli -p sadp-gui --target "$t"
done

lipo -create target/aarch64-apple-darwin/release/sadp-discover \
            target/x86_64-apple-darwin/release/sadp-discover  -output dist/bin/sadp-discover
lipo -create target/aarch64-apple-darwin/release/sadp-discover-gui \
            target/x86_64-apple-darwin/release/sadp-discover-gui -output dist/bin/sadp-discover-gui
```

两个坑：

1. **必须先 lipo 再签名**。先分别签名两个 thin slice 再 lipo，签名会被拼坏（`codesign --verify` 报 `invalid signature`），而且这种坏签名在 arm64 上直接 `Killed: 9`。
2. 用 `vtool -show-build dist/bin/*` 确认两个 slice 的 minos 都是 11.0；不一致的典型症状是 Intel 机器上启动即崩。

### 5.2 组装 bundle（staging 目录，别就地改 target/）

```bash
APP="dist/sadp-discover.app"
rm -rf "$APP"; mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
install -m 755 dist/bin/sadp-discover-gui "$APP/Contents/MacOS/sadp-discover-gui"
install -m 755 dist/bin/sadp-discover     "$APP/Contents/MacOS/sadp-discover"
cp packaging/macos/Info.plist "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"
cp assets/sadp-discover.icns "$APP/Contents/Resources/"
# 图标：assets/sadp-discover.png(1024) -> .iconset -> .icns
# sips -z 16 16 ... && iconutil -c icns AppIcon.iconset -o sadp-discover.icns
```

### 5.3 签名（先内后外，禁用 `--deep`）

```bash
APP_ID="Developer ID Application: Your Name (TEAMID)"
INSTALLER_ID="Developer ID Installer: Your Name (TEAMID)"

codesign --force --options runtime --timestamp --sign "$APP_ID" "$APP/Contents/MacOS/sadp-discover"
codesign --force --options runtime --timestamp --sign "$APP_ID" "$APP/Contents/MacOS/sadp-discover-gui"
codesign --force --options runtime --timestamp --sign "$APP_ID" "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"
```

- `--options runtime`（hardened runtime）是公证的**前置条件**，漏了会被 `notarytool` 直接拒。
- `--deep` 只用于 **verify**，不要用于签名：Apple 已明确它会对嵌套代码乱签，且未来会失效。
- 不需要任何 entitlements：不联网下载可执行代码、不用 JIT、不开 App Sandbox。`com.apple.developer.networking.multicast` 只在上架 Mac App Store 且启用沙盒时才需要。
- 本地无证书时用 `--sign -`（ad-hoc）也能跑通组装与安装流程，只是 Gatekeeper 不认——CI 的 PR 分支就走这条降级路径。

### 5.4 公证 app（先 staple，再进 pkg）

```bash
ditto -c -k --keepParent "$APP" dist/notarize/app.zip
xcrun notarytool submit dist/notarize/app.zip \
  --key "$ASC_KEY_PATH" --key-id "$ASC_KEY_ID" --issuer "$ASC_ISSUER" --wait
xcrun stapler staple "$APP"
spctl -a -vvv -t exec "$APP"        # 必须 accepted / Notarized Developer ID
```

**顺序很关键**：先把票 staple 进 app，再打进 pkg。反过来（只公证 pkg、不 staple app）的话，用户装完断网启动时 Gatekeeper 查不到票，会报「无法验证开发者」。裸 CLI 无法 staple，但它的 CDHash 与 app 内那份相同，在线查票能过。

### 5.5 打 pkg

实现是 `scripts/macos/pkg.sh`，两段式：

```bash
# ① payload 是一棵以 / 为根的树
rm -rf dist/pkgroot && mkdir -p dist/pkgroot/Applications dist/pkgroot/usr/local/bin
ditto --norsrc "$APP" dist/pkgroot/Applications/sadp-discover.app
ditto --norsrc "$APP/Contents/MacOS/sadp-discover" dist/pkgroot/usr/local/bin/sadp-discover

# ② component 包：只有 payload 与安装位置
pkgbuild --root dist/pkgroot --identifier "${SADP_PKG_ID}.pkg" --version "$VERSION" \
  --ownership recommended --install-location / "$work/component.pkg"

# ③ 套上安装向导：欢迎页 / 说明 / 许可协议 / 最低系统版本
productbuild --distribution "$work/Distribution.xml" \
  --package-path "$work" --resources packaging/macos/installer/resources \
  "dist/sadp-discover-$VERSION-macos.pkg"

# ④ 可选：用 Developer ID Installer 签
productsign --sign "$INSTALLER_ID" "$final" "$final.signed"
```

为什么不是一步 `pkgbuild`：只做 component 的话，用户双击看到的是「此软件包将安装…」的白板向导。
`Distribution.xml` 才是那个能写欢迎页、许可协议、最低系统版本、安装位置限制的东西，代价是必须把
component 包再包一层（`product archive`），产物因此是一层壳 + 一个内嵌 pkg。

- 两个证书是**不同的**：`Developer ID Application` 签代码，`Developer ID Installer` 签 pkg。只有前者 `productsign` 会失败。
- `Distribution.xml` 里的 product id 与 `pkg-ref` id 是两种身份：后者必须等于 `pkgbuild --identifier`，写错 `productbuild` 直接报错找不到组件包。
- `<volume-check>` 的 `min="11.0"` 要与 `Info.plist` 的 `LSMinimumSystemVersion` 一起改，否则会出现「装得上、打不开」。
- 注意 `--ownership recommended`，不要出现 root:wheel 以外莫名其妙的属主。
- payload 里的 `._*` 是 `com.apple.provenance` 扩展属性被 pkgbuild 编码成 AppleDouble 的结果，内核对这个属性不允许删除（`xattr -d` 静默失败），属无损噪音。

### 5.6 公证 pkg + dmg

```bash
xcrun notarytool submit dist/sadp-discover-$VERSION-macos.pkg \
  --key "$ASC_KEY_PATH" --key-id "$ASC_KEY_ID" --issuer "$ASC_ISSUER" --wait
xcrun stapler staple dist/sadp-discover-$VERSION-macos.pkg
spctl -a -vvv -t install dist/sadp-discover-$VERSION-macos.pkg
```

dmg 同样走已有的三个模块，不需要单独的命令：

```bash
bash scripts/macos/dmg.sh                              # 组镜像 + 签名 dmg 本身
bash scripts/macos/notarize.sh dist/*.dmg              # 提交 + staple（notarytool 直接收 dmg）
spctl -a -vvv -t open --context context:primary-signature dist/*.dmg
```

三个约束，顺序或载体错了就会得到「能开但没票据」的包：

- **必须在 `bundle.sh` 之后**：镜像里放的是 `dist/sadp-discover.app` 的副本（`ditto` 保留签名元数据）。
- **必须在 app 的 `notarize.sh` 之后**：staple 的票据在 app 内部，先做镜像等于把无票据的 app 封进去；
  只公证 dmg 而不公证 app，断网拖拽安装的用户仍然会被 Gatekeeper 拦。
- **dmg 自己也要签名**（`CODESIGN_IDENTITY` 非空时 `dmg.sh` 自动做）：`spctl` 对 dmg 的判定类型是
  `open` + primary signature，不签就只认公证结果、拿不到 `source=Notarized Developer ID`。

`dmg.sh` 的镜像内容只有两样：`sadp-discover.app` 与指向 `/Applications` 的软链。
不放安装器脚本（用户基本不点，且会被 Gatekeeper 拦），也不写 `/usr/local/bin`——那件事 pkg 负责。

### 5.7 收尾

```bash
shasum -a 256 dist/*.pkg dist/*.dmg dist/*.tar.gz > dist/SHA256SUMS
```

tar.gz 也别漏了签名：`dist/bin/sadp-discover` 已经是签好的、lipo 后的产物，直接 `tar -czf` 即可（`COPYFILE_DISABLE=1` 防止 `._*` 混进去）。

## 6. 打包前必须先改的代码（否则包出来是坏的）

这几条不属于「打包脚本」，但会直接决定包能不能用：

1. ~~**GUI 的失败路径在 Finder 下是静默的**~~：已修。绑定失败与窗口初始化失败都走 `fatal()`——照旧写日志与 stderr，另外在**由 LaunchServices 启动**时（环境变量 `__CFBundleIdentifier` 存在，即 Finder 双击或 `open`）弹 `rfd::MessageDialog`。刻意不用 `isatty(stderr)`：脚本把输出重定向到日志时同样不是 tty，那种场景弹窗会把自动化卡死。
2. **确认没有相对路径依赖**。Finder 启动的 cwd 是 `/`。目前日志走 `dirs`、导出走 `rfd` 默认目录、无外部资源引用，已满足；新增功能时别引入 `./locales` 之类。
3. **`MACOSX_DEPLOYMENT_TARGET` 固定**为 `11.0`，并和 `LSMinimumSystemVersion` 对齐。可以在 xtask/脚本里 export，也可以给 GUI crate 加 `build.rs` 注入——这是 build.rs 的正当用法（编译期决定链接参数），与上一轮讨论一致：**发布动作不进 build.rs，编译参数可以**。
4. **需要新增图标资产**：`assets/sadp-discover.png`（1024×1024），以及生成 `.icns` 的脚本。没有它 app 是通用空白图标，Dock 和「关于」窗口都会很难看。
5. ~~**`InfoPlist.strings` 本地化**~~：已做。`packaging/macos/{zh-Hans,en}.lproj/InfoPlist.strings`，`bundle.sh` 复制进 `Contents/Resources/` 并用 `plutil -lint` 校验（见 §3 末尾）。

## 7. 仓库需要新增的文件

```
packaging/macos/
├── Info.plist                    # ✅ 占位符 __VERSION__ / __BUILD__，bundle.sh 里替换
├── make-icon.py                  # ✅ PNG -> iconset -> icns（Pillow）
├── AppIcon.iconset/              # 生成物，已 gitignore
├── zh-Hans.lproj/
│   └── InfoPlist.strings         # ✅ 中文授权文案
├── en.lproj/
│   └── InfoPlist.strings         # ✅ 英文授权文案
├── installer/
│   ├── Distribution.xml          # ✅ productbuild 的向导定义（占位符由 pkg.sh 替换）
│   └── resources/
│       ├── welcome.html          # ✅ 首屏：装了什么、装到哪
│       ├── readme.html           # ✅ 「安装前请阅读」：权限、PATH、卸载
│       ├── conclusion.html       # ✅ 完成页
│       └── LICENSE.txt           # ✅ 许可协议页（MIT 全文 + Apache-2.0 指向）
└── entitlements.plist            # ⬜ 可选：当前不需要任何 entitlement
assets/
├── sadp-discover.png             # ✅ 1024pt 母版
└── sadp-discover.icns            # ✅ make-icon.py 生成，入库
scripts/macos/
├── config.sh                     # ✅ 版本/target/包名/路径的唯一事实来源
├── build.sh                      # ✅ 构建 + lipo，不签名
├── sign.sh                       # ✅ 只签名
├── notarize.sh                   # ✅ 只公证 + staple
├── package.sh                    # ✅ 归档 + sha256
├── bundle.sh                     # ✅ 组装 .app（Info.plist + 图标 + ad-hoc 签名）
├── pkg.sh                        # ✅ 打 .pkg（app + /usr/local/bin）
└── dmg.sh                        # ✅ 打 .dmg（app + Applications 软链 + 可选签名）
```

新增 `bundle.sh` / `pkg.sh` / `dmg.sh` 时沿用 `config.sh`，不要另起一套变量。

## 8. 与构建系统的关系

沿用上一轮结论：**不要把打包塞进 `build.rs`**（它跑在 crate 编译之前，拿不到最终二进制；且会污染每次 `cargo build`）。

现状已经按职责拆开，耦合点全部收敛：

| 原来耦合在一起的 | 现在解到哪 |
|---|---|
| 版本号 ↔ `Cargo.toml` 的文本格式（`awk` 逐行扫） | `config.sh` 调 `cargo pkgid`，cargo 说什么就是什么 |
| 构建 ↔ 工具链安装（`rustup target add ... \|\| true` 吞错） | `build.sh` 只预检，缺 target 立即失败并给命令；`SADP_INSTALL_TARGETS=1` 才代装 |
| 包名/二进制名 ↔ 散落各处的字面量 | `config.sh` 的 `sadp_bin_for_pkg` 单一映射，加 GUI 只补一行 |
| 产物路径 ↔ `target/` 硬编码 | 从 `cargo metadata` 取 `target_directory`，尊重 `CARGO_TARGET_DIR` |
| 构建 ↔ 签名/公证（凭据逻辑内联，跑构建就可能触发签名） | `sign.sh` / `notarize.sh` 只吃已有产物；未提供凭据即跳过 |
| 归档命名 ↔ 两处重复 | `sadp_tar_name` 一处生成 |
| 平台假设 ↔ 每个步骤 | `uname` 门槛只留在编排入口 |

bundle / pkg / dmg 三个交付形态就是这样加进来的（`bundle.sh` / `pkg.sh` / `dmg.sh`），再要加别的继续往这套模块里加，而不是回到单文件长脚本。**不建议**现在改成 `xtask`：`config.sh` 已经把「事实」和「动作」分开了，迁移到 Rust 只增加代码量，不会消除额外耦合。

CI 拆成两条工作流（`.github/workflows/`）：

- **`dev.yml`**：`push` 到 `main` 与 `pull_request` 触发，只跑 `test`（fmt / clippy / Intel 交叉检查 / 单元测试）与 `e2e`（Windows 假设备），**不产出任何产物**；
- **`release.yml`**：仅 `v*` tag 触发，先过 `version` 闸门与 `cargo-deny`，再构建、签名、公证、打包，最后 `gh release create` 挂产物。

macos job 的每一步都能在本地一对一重放：

| 步骤 | 本地等价命令 |
|---|---|
| 通用 CLI | `TARGETS=aarch64-apple-darwin x86_64-apple-darwin bash scripts/macos/build.sh` |
| 通用 GUI | `PACKAGES=sadp-gui TARGETS=… bash scripts/macos/build.sh` |
| 签名 | `bash scripts/macos/sign.sh dist/sadp-discover dist/sadp-discover-gui` |
| 组装 app | `bash scripts/macos/bundle.sh` |
| 公证 app | `bash scripts/macos/notarize.sh dist/sadp-discover.app` |
| 打 pkg | `bash scripts/macos/pkg.sh` |
| 公证 pkg | `bash scripts/macos/notarize.sh dist/*.pkg` |
| 归档 CLI | `bash scripts/macos/package.sh dist/sadp-discover` |

需要的 secrets：

| secret | 用途 |
|---|---|
| `MACOS_CERT_P12` / `MACOS_CERT_PASSWORD` | Developer ID Application + Installer 的 p12（base64）与密码 |
| `NOTARY_KEY_P8` / `NOTARY_KEY_ID` / `NOTARY_ISSUER` | App Store Connect API Key |

两者的缺失都走降级而不是失败：没有证书 → ad-hoc 签名（仍能装、能跑，过不了 Gatekeeper）；
没有公证密钥 → 跳过公证与 `spctl` 校验。证书与密钥只在「导入」「公证」这两类 step 里注入，
`cargo build` 期间的环境变量里看不到它们——否则任何依赖的 build script 都能读到。

发布前还有两道闸：

- **`version`**：`v1.2.3` 必须等于 `cargo pkgid -p sadp-cli` 的版本，避免「包叫 1.0.0、`--version` 报 0.1.0」；
- **安装冒烟**：runner 上 `sudo installer -pkg … -target /` 装一遍，再跑
  `/usr/local/bin/sadp-discover` 与 app 内的 GUI——这是唯一能验证 pkg 布局真的对的方式。

## 9. 卸载

pkg 没有 uninstall 钩子，文档里给三条命令（写进 README）：

```bash
sudo rm -rf /Applications/sadp-discover.app
sudo rm -f  /usr/local/bin/sadp-discover
rm -rf ~/Library/Application\ Support/sadp-discover
```

TCC 的本地网络授权记录在系统设置里删除（bundle id 见 §3），否则重装后不会重新弹窗。

## 10. 分期与风险

| 阶段 | 内容 | 估时 |
|---|---|---|
| M1 | 图标资产 + Info.plist + staging 组装 + 本地 unsigned 跑通双击 | 0.5 天 |
| M2 | pkg（含 CLI 到 `/usr/local/bin`）+ 卸载文档 | 0.5 天 |
| M3 | 签名 + 公证 + staple + CI secrets，产出「能过 Gatekeeper」的包 | 0.5–1 天 |
| M4 | dmg、SHA256SUMS、release notes、真机验收 8 条 | 0.5 天 |

主要风险：

- ~~**x86_64 的 GUI 从未构建过**~~：已构建并验证。`cargo check --target x86_64-apple-darwin -p sadp-gui` 通过，
  且 universal app 的 x86_64 slice 在 Rosetta 下能正常运行（`arch -x86_64 …/sadp-discover-gui --version`）；
  剩余风险只有「真 Intel 硬件上的窗口/Metal 表现」，托管 runner 与 Rosetta 都覆盖不到。
- **`LSMinimumSystemVersion` 需要真机校准**：写得保守会拒绝本可运行的老系统，写得激进会在老系统上崩。建议先按 11.0 发，收到反馈再调。
- **首次公证会有几轮驳回**：常见原因是 `._*` 扩展属性、未签的嵌套文件、`--deep` 签出的坏签名。把 `notarytool submit` 的 `status: Invalid` 日志当成构建失败来读。
- **pkg 覆盖 `/usr/local/bin` 已有文件**：不提示、直接覆盖。Homebrew 用户混装会坏，靠 release notes 约束。
