# sadp-discover — 架构与分阶段实施路线

> 状态：草案 v0.2 · 2026-11-10
> 范围：Hikvision SADP（Search Active Devices Protocol）设备发现
> 交付形态：① Windows CLI + TUI → ② macOS → ③ GUI（egui）

---

## 1. 目标与范围

**一句话定位**：一个单文件、无运行时依赖、中英双语、支持 IPv4/IPv6 双栈的跨平台 SADP 设备发现工具，既可交互（TUI/GUI）又可脚本化（JSON/CSV）。

### 1.1 已确认的范围

| 决策 | 结论 |
|---|---|
| 三个交付形态 | Windows CLI、macOS、GUI（Linux 不在排期内，架构上可低成本支持） |
| 一阶段能力 | **仅发现**，严格只读；改 IP / 激活 / 重置密码不进入本期 |
| 设备范围 | 仅海康（Hikvision）SADP 设备 |
| 地址族 | 必须支持 IPv4、IPv6-only、双栈；同一设备跨栈只出现一次 |
| 语言 | 中文 + 英文，可运行时切换 |
| Windows 版本 | 暂不设下限，按 Win10 1809+ 验证 |

### 1.2 一阶段交付的"完整搜索"

| 能力 | 说明 |
|---|---|
| 主动探测 | IPv4：组播 `239.255.255.250:37020` + 各网卡子网广播；IPv6：链路本地组播（组地址待抓包确认，默认候选 `ff02::1`，可 `--v6-mcast` 覆盖） |
| 被动监听 | 绑定 37020 加入组播组，接收设备上电/心跳时主动通告的 ProbeMatch（`--passive`） |
| 多网卡 | 枚举全部 IPv4/IPv6 接口，逐网卡建 socket、设置组播出口、join group，避免只走默认路由 |
| 双栈去重 | 设备可能同时从 v4 与 v6 应答 → 按 SN → MAC → IP 合并为一条记录，标注"经 v4 / v6 / 双栈发现" |
| IPv6-only | 主机禁用 IPv4 协议栈、或设备仅有 IPv6 地址时仍能发现（协议可行性见 §2.4） |
| 结果聚合 | 同一设备多包响应逐字段合并（非空覆盖 + 首末次时间），保留来源网卡与地址族 |
| 实时 TUI | 表格 + 详情 + 过滤 + 排序 + 连续扫描/暂停/重扫 + 导出 + 复制 |
| 中英双语 | CLI 帮助、TUI、GUI、错误与日志提示全部可切换；JSON/CSV 字段名保持英文 |
| 脚本输出 | `--json` / `--csv` / `--quiet`，稳定 schema，退出码可判定 |
| 诊断 | `interfaces` 子命令（列出 v4/v6 地址与组播加入结果）、`--bind/--interface/--port/--timeout/--repeat/--family/--v6-mcast` |

### 1.3 非目标

- 修改 IP / 子网 / 网关，设备激活，密码重置，安全码导出（写路径，留作四阶段）。
- CSADP（二进制变体）、ONVIF/WS-Discovery、大华/宇视等其它厂商协议。
- Linux 发行包（不排期）。
- 跨三层网段发现：SADP 设备**只监听组播/广播**，单播到设备 37020 通常被拒绝（Linux 上 `connection refused`，Windows 上静默丢弃），所以跨网段不是"加个 unicast 兜底"能解决的，需要组播路由或二层打通。

### 1.4 阶段总览

| 阶段 | 形态 | 交付物 | 预估 |
|---|---|---|---|
| 一 | Windows · CLI + TUI | `sadp-discover.exe`，只读发现（v4/v6/双栈）+ 中英双语 | ~3 周 / 15–16 人日 |
| 二 | macOS · CLI + TUI | 通用二进制、权限与签名公证链路 | 4–5 天 |
| 三 | Windows + macOS · GUI | `sadp-discover-gui`（egui），功能对齐 TUI | 1.5–2 周 |

### 1.5 非功能指标

- 单 exe，除 MSVC 运行时外无外部依赖（可静态链接 CRT）。
- 冷启动 < 100 ms；100 台设备、10 Hz 刷新 CPU < 5%。
- 扫描窗口 3 s 内出首屏；连续扫描 24 h 无句柄/内存增长。
- 双语界面在 80×24 终端下不截断、不串行。
- **只读**：一阶段任何代码路径都不向设备发送写指令。

---

## 2. 协议事实（决定架构的部分）

SADP 是海康私有的 UDP 发现协议，报文为 UTF-8 XML，端口 **37020**。它不是 ONVIF/WS-Discovery，只是复用了同一个 IPv4 组播地址。

### 2.1 报文

请求 `Probe`：

```xml
<?xml version="1.0" encoding="utf-8"?>
<Probe>
  <Uuid>0f8fad5b-d9cb-469f-a165-70867728950e</Uuid>
  <Types>inquiry</Types>
</Probe>
```

响应 `ProbeMatch`（字段随固件有增减）：

| 字段 | 含义 | 备注 |
|---|---|---|
| `Uuid` | 设备会话 UUID | 每次探测可不同，**不能**当设备唯一键 |
| `Types` | `inquiry` | 写操作时会是其它值 |
| `DeviceType` | 型号，如 `DS-2CD2T47G2-L` | |
| `DeviceDescription` | 设备描述/长名称 | 用户可改，可能含空格与中文 |
| `DeviceSN` | 序列号 | 设备唯一，但老固件可能缺 |
| `CommandPort` | SDK 端口，通常 8000 | |
| `HttpPort` | Web 端口，通常 80 | |
| `MAC` | 有线 MAC | 去重首选键 |
| `IPv4Address` / `IPv4SubnetMask` / `IPv4Gateway` | 网络配置 | 部分老固件用 `<DeviceAddress>` 包裹，需兼容两种形态 |
| `IPv6Address` / `IPv6Masklen` / `IPv6Gateway` | IPv6 配置 | 可能为 `::`；可能是链路本地地址 |
| `DHCP` | 是否 DHCP | `true/false`，偶见 `1/0` |
| `DSPVersion` | 固件版本 | 字段名历史遗留 |
| `Activated` | 是否已激活 | 老固件无此字段 → `Option<bool>` |
| `PasswordResetAbility` | 是否支持重置 | |
| `AnalogChannelNum` / `DigitalChannelNum` | 通道数 | |
| `Support` | 能力位串 | 语义不明，原样保留 |
| `DeviceAddress` | 老固件的嵌套网络字段 | 兼容分支 |

### 2.2 行为特征

1. 探测包发往组播组；设备可**单播回给源地址**，也可**再组播一份**到同一组。因此本机要同时能收单播与组播。
2. 官方 SADP 工具绑定本地 **37020** 端口收发；部分老设备只回源端口 37020 的请求，故默认也绑 37020（失败再退到临时端口并告警）。
3. 设备**只监听组播/广播**，直接单播到 `<设备IP>:37020` 一般无响应，不能作为主要发现手段。
4. 设备上电/心跳时可能主动向组播组通告，故存在"不探测也能发现"的被动模式。
5. 报文很小（< 2 KB），但读取缓冲区仍按 64 KiB 上限截断，防伪造大包。
6. UDP 无认证，**响应可被伪造**：结果仅用于发现与展示，不得作为安全决策依据。

### 2.3 兼容性坑位

- XML 声明、BOM、大小写、空白、未知字段、字段顺序都不稳定 → 解析必须宽松，未知字段进 `extra` 保留。
- 同一设备不同响应字段完整度不同 → 必须"逐字段合并"而非"覆盖整条记录"。
- 中文/特殊字符的 `DeviceDescription` 直接进终端会引发 ANSI 注入 → 展示前剥离控制字符。
- Windows 多网卡（Hyper-V、VPN、WSL、虚拟网卡）会让"只发默认路由"的方案漏设备 → 逐网卡发送是硬要求。

### 2.4 IPv6 的已知与未知（必须先验证）

**已知**

- `ProbeMatch` 携带 `IPv6Address/IPv6Masklen/IPv6Gateway`，即设备经 IPv4 被发现时也会上报自己的 IPv6 配置。
- 海康设备支持 IPv6/双栈配置；IPv6-only 设备没有 IPv4 地址。

**未知（不能拍脑袋写死）**

- 是否存在 IPv6 的 SADP 探测通道：组播组地址、端口是否仍为 37020、报文是否同一份 XML。
- 官方 SADPTool 在 IPv6-only 环境下是否有发现能力（有则抓它的包即可定论）。

**验证方法（M2b 预研，产出协议事实）**

1. 设备配置为 IPv6-only，PC 与设备同二层；Wireshark 抓 `udp.port == 37020` 与 ICMPv6 MLD（`icmpv6.type >= 130`）。
2. 运行官方 SADPTool，观察是否发出 IPv6 报文；同时看设备加入了哪些组播组（MLDv2 报告里能直接读到）。
3. 用测试程序向候选组（`ff02::1`、以及 MLD 里发现的组）发送 Probe，看是否有 `ProbeMatch` 回包。
4. 结论固化：默认组地址、是否需要 scope、是否支持单播回包。

**降级策略（若设备固件根本不响应 IPv6 探测）**

- 仍展示 IPv4 发现设备的 IPv6 地址（该能力不受影响）。
- 尝试 169.254/16 链路本地广播（部分设备保留 v4 link-local）。
- 明确告知用户"纯 IPv6 且不响应 v6 探测的设备无法在应用层发现"，不制造虚假能力。

> 架构上把地址族做成传输层参数，协议一旦确认只需改默认值，不动上层。

---

## 3. 总体架构

### 3.1 设计原则

1. **协议与 IO 分离**：编解码是纯函数，可单测、可 fuzz，不碰 socket。
2. **引擎与界面分离**：扫描引擎只产出事件与快照；TUI 与 GUI 都只是消费者，三阶段复用一阶段成果。
3. **平台差异收敛**：所有 `setsockopt`、网卡枚举、权限差异只出现在 `sadp-net`，上层零 `cfg(target_os)`。
4. **地址族参数化**：v4/v6 是同一套接口的两种目标，不是两套代码路径。
5. **同步优先**：UDP 小流量不引入 async runtime；线程 + channel 足够，`Transport` trait 保留替换余地。
6. **文案与代码分离**：所有面向用户的字符串走 key，禁止在 UI 代码里写死中文或英文。

### 3.2 分层与数据流

```
Probe (XML)  ── v4: 239.255.255.250 / 子网广播
             └─ v6: ff02::1 (候选，待验证)
   │
   ▼
sadp-net    网卡枚举(v4/v6) / 每网卡每族 socket / 组播·广播 / 接收线程 → channel
   │  Datagram { bytes, src, iface, family }
   ▼
sadp-core   解码 ProbeMatch → DeviceStore（跨栈去重 / 合并 / 老化）→ ScanEvent
   │  ScanEvent / &DeviceStore
   ├──▶ sadp-cli（TUI，中/英）
   └──▶ sadp-gui（egui，中/英）
```

### 3.3 线程模型（一阶段）

| 线程 | 数量 | 职责 |
|---|---|---|
| UI 主线程 | 1 | 终端事件（crossterm `poll`）、渲染、把 `ScanEvent` 应用到 `AppState` |
| 接收线程 | 每网卡每地址族 1 | 阻塞 `recv_from`（读超时 200 ms），把数据报塞进 `crossbeam_channel` |
| 扫描调度 | 1 | 按 `--repeat` 周期发 Probe；一轮结束后发 `RoundFinished` |

单地址族创建失败（例如主机禁用 IPv4 协议栈时 `WSAEAFNOSUPPORT`）只降级该族，不影响整体启动。设备数在千级以内，无需 epoll/kqueue 抽象。

---

## 4. 工程结构

```
sadp-discover/
├─ Cargo.toml                 # [workspace] members = [...]
├─ rust-toolchain.toml        # 固定 stable，声明 MSRV
├─ crates/
│  ├─ sadp-net/               # 平台传输层；socket 相关的 cfg(target_os) 只在这里
│  │  └─ src/{lib,iface,udp,mock}.rs
│  ├─ sadp-core/              # 协议 + 扫描引擎 + i18n + 系统工具调用（无终端/无 GUI）
│  │  ├─ locales/{en-US,zh-CN}.yml
│  │  └─ src/{lib,proto/{model,parse,encode},scan/{config,session,store,event},view,i18n,system}.rs
│  ├─ sadp-cli/               # 一阶段产物
│  │  └─ src/{main,args,commands,tui,output,logging,clipboard}.rs
│  └─ sadp-gui/               # 三阶段产物
├─ xtask/                     # 构建/打包/假设备（fake-device）
├─ tests/fixtures/            # 抓包得到的 XML 样本（脱敏）
├─ docs/adr/                  # 决策记录
└─ .github/workflows/         # windows / macos 矩阵
```

依赖方向（严格单向）：

```
sadp-cli ──▶ sadp-core ──▶ sadp-net
sadp-gui ──▶ sadp-core ──▶ sadp-net
```

`Transport` trait 定义在 `sadp-net`，`sadp-core` 只依赖 trait，单测注入 `MockTransport`，不需要真网卡。`sadp-cli`/`sadp-gui` 互不依赖。i18n 放在 `sadp-core`，两套 UI 共用同一份 key 与翻译文件。

---

## 5. Crate 预选

> 版本为 2026-11 查询到的 crates.io 最新稳定版；落地时锁定 `Cargo.lock`，升级走 dependabot。

### 5.1 选定

| 用途 | crate | 版本 | 阶段 | 选型理由 |
|---|---|---|---|---|
| 参数解析 | `clap`（derive + `env` + `wrap_help`） | 4.6 | 1 | 事实标准；帮助文本可运行时注入，便于 i18n |
| shell 补全 | `clap_complete` | 4.x | 1 | 生成 pwsh/bash/zsh 补全 |
| TUI | `ratatui` | 0.30 | 1 | 社区活跃、布局/表格/滚动成熟；替代已停更的 `tui-rs` |
| 终端后端 | `crossterm` | 0.29 | 1 | 跨平台事件与 raw mode；ratatui 一等支持 |
| 过滤输入框 | `tui-input` | 0.15 | 1 | 处理光标/编辑键（可选） |
| XML 解析/生成 | `quick-xml` | 0.42 | 1 | 快、流式；手写 Reader 比 serde 更能容忍脏数据 |
| 序列化 | `serde` + `serde_json` | 1.x | 1 | `--json` 输出与配置 |
| CSV 导出 | `csv` | 1.4 | 1 | serde 直出，转义正确 |
| Socket 控制 | `socket2` | 0.6 | 1 | 组播 join/出口、`only_v6`、`SO_REUSEADDR` 等 std 不暴露的选项 |
| 网卡枚举 | `if-addrs` | 0.15 | 1 | 跨平台返回 name/index、v4 的 netmask/broadcast、v6 地址 |
| 线程通信 | `crossbeam-channel` | 0.5 | 1 | 多生产者单消费者 + 超时，比 std mpsc 灵活 |
| 国际化 | `rust-i18n` | 4.2 | 1 | YAML 编译期嵌入、`t!` 宏、运行时切换 locale，轻量 |
| 系统语言检测 | `sys-locale` | 0.3 | 1 | 决定默认语言（`--lang` > 环境变量 > 配置 > 系统） |
| 库错误 | `thiserror` | 2.0 | 1 | 库层结构化错误 |
| 应用错误 | `anyhow` | 1.x | 1 | 仅 bin 层，带上下文 |
| 日志 | `tracing` + `tracing-subscriber`（`env-filter`） | 0.1 / 0.3 | 1 | 结构化、分级、可落文件；TUI 下不污染终端 |
| 日志落盘 | `tracing-appender` | 0.2 | 1 | 非阻塞写入 |
| 时间格式化 | `time`（`formatting` + `local-offset`） | 0.3 | 1 | 比 chrono 轻；内部用 `Instant` 算时长 |
| 配置/日志路径 | `dirs` | 6.x | 1 | `%LOCALAPPDATA%` / `~/Library/Application Support` |
| GUI 框架 | `eframe` + `egui` | 0.37 | 3 | 即时模式、单二进制、Win/mac 一致 |
| GUI 表格 | `egui_extras`（`TableBuilder`） | 0.37 | 3 | 虚拟化行、列宽可调 |
| 文件对话框 | `rfd` | 0.15 | 3 | 原生 Win32/Cocoa 对话框 |
| CJK 字体（资源，非 crate） | Noto Sans SC 子集（OFL） | — | 3 | **egui 默认字体无中文字形**，必须自带；用 `pyftsubset` 裁到常用字 |
| 快照测试 | 未引入（`insta` 曾列入选型） | — | — | 现用 `TestBackend` 文本断言 + JSON 字段断言，见 §13.3 |
| 属性测试 | `proptest` | 1.12 | 1 | 解析器不 panic |
| CLI 集成测试 | `assert_cmd` + `predicates` + `tempfile` | 2.x | 1 | 黑盒验证退出码/stdout |
| 模糊测试 | `cargo-fuzz`（libFuzzer，nightly） | — | 1 | 解析器吃任意字节不崩 |
| 打包（CLI） | `cargo-dist` | 0.3x | 1/2 | 出 zip/msi/tar.gz + GitHub Release |
| 打包（GUI） | `cargo-bundle`（mac .app）/ `cargo-wix`（msi） | — | 3 | 或统一交给 cargo-dist |
| macOS 底层 | `libc`（必要时 `nix`） | — | 2 | `IP_BOUND_IF` 等 socket2 未覆盖的选项 |

### 5.2 明确不选

| 候选 | 不选原因 |
|---|---|
| `tokio` / `async-std` | 小包低频场景，async 只增加依赖与跨平台调试成本；trait 已留替换位 |
| `pnet` / `pcap` | 需要 Npcap 驱动，破坏"单 exe 开箱即用"；我们要的是普通 UDP socket |
| `serde-xml-rs` / `xml-rs` 高层封装 | 维护弱、对未知字段/大小写/缺失字段过于严格 |
| `fluent` / `i18n-embed` | 复数/性别规则更强，但本项目字符串量级用不上，配置成本更高；若将来要 5+ 语言再迁移 |
| `gettext-rs` | 依赖系统 gettext 运行库，与单文件分发冲突 |
| `tui-rs` | 已归档 |
| `windows-sys`（默认） | 只有必须 `WSAIoctl` 时才引入，能用 socket2 就不引 |
| `chrono` | 仅格式化场景偏重，`time` 足够 |

---

## 6. 核心接口草图

### 6.1 传输层

```rust
// sadp-net/src/lib.rs
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Family { V4, V6 }

pub struct InterfaceInfo {
    pub name: String,                  // "以太网 2" / "en0"
    pub index: u32,                    // 组播出口用（v6 必填）
    pub ipv4: Option<Ipv4Addr>,
    pub netmask: Option<Ipv4Addr>,
    pub broadcast: Option<Ipv4Addr>,   // 子网广播，优先于 255.255.255.255
    pub ipv6: Vec<Ipv6Addr>,           // 含 fe80:: 链路本地
}

pub struct Datagram {
    pub bytes: Vec<u8>,
    pub src: SocketAddr,               // v6 链路本地会带 scope_id
    pub iface: InterfaceInfo,
    pub family: Family,
}

pub trait Transport: Send + Sync {
    fn interfaces(&self) -> &[InterfaceInfo];
    fn send_probe(&self, families: &[Family], payload: &[u8]) -> Result<(), NetError>;
    /// 阻塞至多 timeout，取一条数据报；内部由各接收线程汇入 channel
    fn recv(&self, timeout: Duration) -> Result<Option<Datagram>, NetError>;
}
```

### 6.2 协议模型（含双栈地址）

```rust
// sadp-core/src/proto/model.rs
pub struct Ipv6Endpoint {
    pub addr: Ipv6Addr,
    pub scope_id: Option<u32>,   // 链路本地地址必须带 scope 才能连接/展示
    pub global: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Device {
    pub sn: Option<String>,
    pub mac: Option<MacAddr>,
    pub ipv4: Option<Ipv4Addr>,
    pub netmask: Option<Ipv4Addr>,
    pub gateway: Option<Ipv4Addr>,
    pub ipv6: Vec<Ipv6Endpoint>,
    pub ipv6_masklen: Option<u8>,
    pub dhcp: Option<bool>,
    pub model: Option<String>,        // DeviceType
    pub description: Option<String>,  // DeviceDescription
    pub firmware: Option<String>,     // DSPVersion
    pub sdk_port: Option<u16>,        // CommandPort
    pub http_port: Option<u16>,
    pub activated: Option<bool>,
    pub reset_ability: Option<bool>,
    pub analog_channels: Option<u16>,
    pub digital_channels: Option<u16>,
    pub support: Option<String>,
    pub extra: BTreeMap<String, String>,
    #[serde(skip)] pub first_seen: Instant,
    #[serde(skip)] pub last_seen: Instant,
    #[serde(skip)] pub seen_via: SeenVia,   // v4 / v6 / both
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeenVia { V4, V6, Both }

pub fn decode_probe_match(bytes: &[u8]) -> Result<ProbeMatch, DecodeError>;
pub fn encode_probe(uuid: Uuid) -> Vec<u8>;
```

### 6.3 扫描引擎

```rust
pub struct ScanConfig {
    pub port: u16,                       // 37020
    pub timeout: Duration,               // 单轮窗口，默认 3s
    pub repeat: Option<Duration>,        // None = 单轮
    pub families: Vec<Family>,           // auto = [V4, V6]
    pub v6_mcast: Ipv6Addr,              // 预研前默认 ff02::1，可配置
    pub broadcast: bool,
    pub interfaces: InterfaceSelector,
    pub passive: bool,
}

/// Auto = 全部可用网卡；Only(names) = 用户显式指定
pub enum InterfaceSelector { Auto, Only(Vec<String>) }

pub enum ScanEvent {
    RoundStarted { id: u64, at: Instant },
    DeviceFound(Device),
    DeviceUpdated { key: DeviceKey, changed: Vec<&'static str> },
    DecodeFailed { src: SocketAddr, error: String },
    NetError(NetError),
    RoundFinished { id: u64, found: usize },
}

/// 形如 "sn:DS-2CD..." / "mac:a4:14:37:.." / "ip:192.168.1.64"
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeviceKey(pub String);
```

去重键优先级：`SN`（稳定且唯一）→ `MAC` → `IPv4` → `IPv6(global)`。**MAC 是跨栈合并的关键**：同一设备经 v4 与 v6 各回一包时，靠 MAC/SN 合并成一条，`seen_via` 记为 `Both`。

### 6.4 socket 初始化（双栈，关键实现）

```rust
// IPv4：每网卡一个 socket
let s4 = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
s4.set_reuse_address(true)?;
s4.set_broadcast(true)?;
s4.bind(&SocketAddr::from((iface.ipv4.unwrap(), port)).into())?;
s4.set_multicast_if_v4(&iface.ipv4.unwrap())?;
s4.set_multicast_loop_v4(true)?;
s4.set_multicast_ttl_v4(1)?;
s4.join_multicast_v4(&MCAST_V4, &iface.ipv4.unwrap())?;

// IPv6：显式 only_v6，否则会与 v4 抢 37020
let s6 = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
s6.set_only_v6(true)?;
s6.set_reuse_address(true)?;
s6.bind(&SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)).into())?;
s6.join_multicast_v6(&MCAST_V6, iface.index)?;   // 链路本地组必须带 ifindex
s6.set_multicast_if_v6(iface.index)?;
s6.set_multicast_hops_v6(1)?;
s6.set_multicast_loop_v6(true)?;
```

发送顺序：v4 组播 → v4 子网广播 → v6 链路本地组播。
绑定 37020 失败时：记录 `Warn`，退回临时端口，功能降级但可用。
单族初始化失败：记 `NetError`，另一族继续。

### 6.5 UI 事件循环（TUI 与 GUI 共用形态）

```rust
loop {
    for ev in scanner.poll(Duration::from_millis(100)) { app.apply(ev); }
    terminal.draw(|f| ui::draw(f, &app))?;
    // gui: 事件线程触发 ctx.request_repaint()，update() 内 drain 同一批事件
    if event::poll(Duration::ZERO)? { app.on_key(event::read()?); }
    if app.should_quit { break; }
}
```

100 ms 轮询同时充当渲染节拍：包风暴不会导致逐包重绘。

### 6.6 国际化

```rust
// sadp-core/src/i18n.rs
rust_i18n::i18n!("locales", fallback = "en-US");

pub fn init(user_pref: Option<&str>) {
    let raw = user_pref
        .map(str::to_owned)
        .or_else(|| std::env::var("SADP_LANG").ok())
        .or_else(sys_locale::get_locale)
        .unwrap_or_else(|| "en-US".into());
    rust_i18n::set_locale(&normalize(&raw));   // zh* → zh-CN, en* → en-US
}

fn normalize(tag: &str) -> String {
    if tag.to_ascii_lowercase().starts_with("zh") { "zh-CN".into() } else { "en-US".into() }
}

pub fn toggle() { /* TUI 里按 L 键、GUI 里下拉框，都调这里 */ }
```

翻译文件按界面分命名空间：

```yaml
# locales/zh-CN.yml
app.name: sadp-discover
cli.about: 发现局域网内的海康设备
cli.scan.timeout: "扫描时长（毫秒，默认 %{default}）"
tui.col.ip: IP 地址
tui.status.scanning: "扫描中… 已发现 %{count} 台"
error.net.bind: "无法绑定 UDP 端口 %{port}：%{reason}"
```

约定：

- **CLI 帮助**：先用 `std::env::args()` 预扫 `--lang`/`SADP_LANG` 决定语言，再构建 `clap::Command` 并用 `t!` 填 `.about()/.help()`（`clap` 支持运行时字符串）。
- **TUI**：`L` 键切换语言并立即重绘；表格列宽按当前语言重算。
- **GUI**：设置里下拉切换，`set_locale` + `request_repaint` 即可，egui 即时模式天然支持。
- **不翻译**：JSON 字段名、CSV 表头、日志 `target`/字段名（保证脚本与检索稳定）；人读的消息体翻译。
- **兜底**：缺失 key 时 `rust-i18n` 回退 `en-US`，并在 debug 构建打警告。

---

## 7. 阶段路线

### 一阶段：Windows CLI + TUI（约 3 周 / 15–16 人日）

| 里程碑 | 内容 | 产出 | 预估 |
|---|---|---|---|
| M0 骨架 | workspace、`rust-toolchain.toml`、CI（fmt/clippy/test）、`cargo-deny` | 空壳可编译 | 0.5 d |
| M1 协议层 | 模型、宽松解析、Probe 编码、golden fixtures、fuzz target | `sadp-core::proto` + 单测 | 2 d |
| M2 传输层（v4） | 网卡枚举、逐网卡 socket、组播/广播、MockTransport、假设备 | `sadp-net` + 回环集成测试 | 2 d |
| **M2b IPv6 预研** | 抓包定组地址/端口/报文；测 IPv6-only 主机与设备；实现 v6 收发 | 一页协议结论 + `--v6-mcast` | 2 d |
| M3 引擎 | 跨栈去重/合并/老化、轮次与超时、事件流、配置 | `sadp-core::scan` | 2 d |
| M4 TUI | 表格/详情/过滤/排序/快捷键/主题/快照测试 | `sadp-cli tui` | 2.5 d |
| M5 CLI | 子命令、`--json/--csv`、退出码、补全、日志落盘 | `sadp discover` 等 | 1 d |
| **M5b 双语** | locale 文件、clap 帮助、TUI 文案、key 对齐测试 | 中/英全量 | 2 d |
| M6 验证发布 | 真机矩阵（v4/v6/双栈）、多网卡/防火墙、文档、cargo-dist、v0.1.0 | release 产物 | 2 d |

**M2b 是唯一带不确定性的里程碑**：若抓包证明设备不支持 IPv6 探测，则把"IPv6-only 发现"降级为"展示 IPv6 地址 + 文档说明"，并立即同步，而不是拖到 M6 才暴露。

**TUI 布局**

```
 sadp-discover   iface: 以太网   round #12   3 devices   1.2s
 IP                          MAC                Model           Via   Act
 192.168.1.64                44:19:b6:xx:xx:xx  DS-2CD2T47G2-L  v4    ✓
 fe80::21b:2bff:fe3c:4d5e    44:19:b6:xx:xx:xx  DS-2CD2T47G2-L  v6    ✓
 192.168.1.65                a4:14:37:xx:xx:xx  DS-7608N-K2     both  ✓
 ── 详情 ─────────────────────────────────────────────────────────
 SN / 网关 / DHCP / SDK·HTTP 端口 / 通道数 / 首次·最近发现 / 来源网卡
 ── [s] 扫描 [空格] 暂停 [r] 清空重扫 [/] 过滤 [e] 导出 [L] 语言 [q] 退出
```

**验收门槛（Gate 1）**

1. 3 种以上真机（IPC/NVR/球机各一）在 Win10、Win11 上默认参数即可发现。
2. 双网卡 + Hyper-V 虚拟网卡环境不漏设备；VPN 未连接时不误报。
3. **双栈**：同一台设备经 v4 与 v6 各回一包时只显示一行，`Via` 为 `both`。
4. **IPv6-only 主机**（禁用 IPv4 协议栈）程序能启动并完成扫描，不因 v4 socket 创建失败而退出。
5. 防火墙开启时首次运行有明确提示，且允许后有结果。
6. `--json` schema 冻结为 `sadp.discover/v1`，带 `insta` 快照测试。
7. `--lang en-US` 与 `--lang zh-CN` 全界面无写死文案、无截断；key 对齐测试通过。
8. 连续扫描 24 h 内存/句柄稳定；单测覆盖 `sadp-core` ≥ 80%。
9. 与官方 SADP 工具同时运行不互相踢端口。

**退出码约定**：`0` 发现设备、`1` 未发现、`2` 参数或网络错误、`3` 权限被拒（防火墙/提权）。

### 二阶段：macOS 适配（约 4–5 人日）

原则：**不改架构，只改 `sadp-net` 的平台模块与打包链路**。

| 工作项 | 要点 |
|---|---|
| 网卡/组播 | `if-addrs` 直接可用；逐网卡 `IP_MULTICAST_IF` + join 与 Windows 逻辑一致 |
| 绑定接口 | socket2 的 `bind_device` 仅 Linux；macOS 需 `IP_BOUND_IF`（`libc::setsockopt`，传 `ifindex`），或依赖"绑到接口地址"的等价效果，二者 A/B 验证 |
| IPv6 链路本地 | macOS 上 scope_id 语义与 Windows 略有差异，需真机回归 |
| 本地网络权限 | macOS 15+ 首次发包触发"本地网络"隐私弹窗；CLI 继承终端授权，GUI 需 `Info.plist` 的 `NSLocalNetworkUsageDescription` |
| 沙盒/分发 | 仅 App Store 沙盒需要 `com.apple.developer.networking.multicast`；Developer ID 分发不需要，但要签名 + 公证 |
| 通用二进制 | `aarch64-apple-darwin` + `x86_64-apple-darwin` 分别构建后 `lipo`，或用 cargo-dist 的 universal 目标 |
| CI | `macos-15` / `macos-latest`（arm64）跑测试 + `x86_64-apple-darwin` 交叉检查；通用二进制构建 |
| 路径 | 配置/日志走 `~/Library/Application Support/sadp-discover/` |

**Gate 2**：macOS 14/15、Apple Silicon 与 Intel 各一台，clean 环境下 TUI 全功能（含中英文切换）可用；首次权限弹窗有文档说明。

### 三阶段：egui 图形界面（约 1.5–2 周 / 8–9 人日）

| 里程碑 | 内容 | 预估 |
|---|---|---|
| G0 状态复用 | 排序/过滤/选择/导出抽到 `sadp-core::view`（纯函数 + 单测），TUI 改为调用它 | 1 d |
| G1 壳与扫描 | `eframe` 窗口、后台扫描线程、事件 drain、`ctx.request_repaint()` | 1.5 d |
| G2 表格与详情 | `egui_extras::TableBuilder` 虚拟化表格、列宽、排序、多选、详情面板、原始 XML | 2 d |
| G3 设置与导出 | 网卡/地址族多选、超时/端口、JSON/CSV 导出（`rfd`）、剪贴板复制 | 1.5 d |
| G4 体验 | 深色主题、状态栏、错误 toast、语言切换、中文字体嵌入与回退 | 1 d |
| G5 打包 | Windows MSI（cargo-wix/cargo-dist）、macOS .app（cargo-bundle）+ 签名公证 | 1–2 d |

GUI 只读同一份 `DeviceStore`，复用同一套 `ScanEvent` 与 locale 文件；主要新增工作是**中文字体嵌入**（egui 自带字体无 CJK，需注册 Noto Sans SC 子集，并处理字体回退顺序）。

---

## 8. 测试与质量

| 层级 | 手段 |
|---|---|
| 编解码 | 真实抓包 fixtures（脱敏）+ 缺失字段/乱序/BOM/非法 UTF-8/超大包 |
| 健壮性 | `cargo-fuzz` 对 `decode_probe_match` 跑 24h；`proptest` 保证任意字节不 panic |
| 引擎 | `MockTransport` 注入脚本化数据报，验证跨栈去重、合并、超时、乱序 |
| 集成 | `xtask fake-device`：本机同时绑定 v4 `127.0.0.1:37020` 与 v6 `[::1]:37020` 回 ProbeMatch，CI 无硬件也能跑双栈端到端 |
| TUI | `ratatui::backend::TestBackend` 文本断言，中/英两种语言 × 80×24 / 40×10；快捷键栏宽度 ≤80 列 |
| i18n | key 对齐测试（中英 key 集合必须一致，CI 失败）；`--lang` 冒烟；硬编码文案扫描（`grep` 中文/英文句子规则） |
| CLI | `assert_cmd` 校验 stdout JSON schema 与退出码 |
| 跨平台 | GitHub Actions 矩阵；Windows 额外跑"37020 被占用"降级用例与"仅 IPv6"用例 |
| 静态 | `clippy -D warnings`、`rustfmt --check`、`cargo-deny` |

CI 关键点：假设备播放的报文与真机结果走**同一套断言**，防止"只在真机验证"的回归。

---

## 9. 风险登记

| # | 风险 | 影响 | 缓解 |
|---|---|---|---|
| R1 | IPv6 探测通道无公开资料，抓包也可能证明设备不支持 | 高 | M2b 前置预研并尽早给结论；`--v6-mcast` 可配置；准备"仅展示 IPv6 地址"的降级方案 |
| R2 | Windows 多网卡/组播行为不一致，漏设备 | 高 | 逐网卡 socket + 组播/广播；真机矩阵；`interfaces` 诊断子命令 |
| R3 | 固件字段差异导致解析失败 | 高 | 宽松解析 + `extra` 保留 + 原始报文落盘开关 + 真实 fixtures |
| R4 | 双栈导致同设备重复展示 | 中 | SN/MAC 合并 + `seen_via` + 专门的双栈集成用例 |
| R5 | 与官方 SADP 争抢 37020 | 中 | `SO_REUSEADDR`；失败降级临时端口并告警 |
| R6 | 防火墙/杀软拦截（v4 与 v6 规则分开） | 中 | 首启提示、错误码、文档；不自动改规则 |
| R7 | 终端 ANSI 注入 | 中 | 展示前剥离 `ESC`/C0/C1，按 `unicode-width` 截断 |
| R8 | 中文字符宽度导致 TUI 错位 | 中 | 统一用 `unicode-width` 计算列宽；中英双语快照测试 |
| R9 | egui 无中文字形 | 中 | 内嵌 Noto Sans SC 子集，字体回退链固定 |
| R10 | 响应伪造 | 中 | 只读定位；文档明示不可用于安全判断 |
| R11 | macOS 本地网络权限/公证 | 中 | 二阶段独立排期；clean 机器验证 |
| R12 | 三阶段 UI 逻辑重复 | 中 | G0 先抽 `view`，两套 UI 共用 |

---

## 10. 决策记录（ADR 摘要）

- **ADR-001 同步线程而非 async**：规模小、依赖少、跨平台行为确定；`Transport` trait 保留替换空间。
- **ADR-002 手写 quick-xml Reader 而非 serde**：需要容忍未知字段、大小写、缺失节点，并保留原始内容。
- **ADR-003 每网卡每族一 socket**：Windows 上单 socket 无法可靠控制组播出口；v6 必须显式 `only_v6`。
- **ADR-004 `sadp-core` 不依赖任何 UI/终端库**：保证三阶段复用与可测试性。
- **ADR-005 JSON schema 版本化**：`sadp.discover/v1`，字段只增不删，删除走 v2；字段名不本地化。
- **ADR-006 地址族参数化**：v4/v6 是同一接口的两种目标，不做两套实现。
- **ADR-007 文案 key 化 + `rust-i18n`**：编译期嵌入、零运行时文件依赖；不用 gettext 以免破坏单文件分发。

---

## 11. 已确认与待确认

**已确认**

1. 三个交付形态：Windows CLI、macOS、GUI；Linux 不排期。
2. 一阶段仅发现，严格只读。
3. 仅海康设备。
4. 需要 IPv6-only 与双栈发现；需要中英文；Windows 版本暂不设限（按 Win10+ 验证）。

**待确认**

1. 是否有可用的 **IPv6-only 测试环境**（一台设备 + 一台禁用 IPv4 的主机）？这直接决定 M2b 能否按期收敛。
2. 默认语言策略：跟随系统，还是无配置时默认中文？
3. 分发形态：绿色单文件优先，还是需要 MSI/安装包与自动更新？
4. 设备固件范围：哪些代际/型号必须支持（决定 fixtures 与真机矩阵规模）。
5. 链路本地 IPv6 地址在 UI 上如何展示（是否带 `%iface` scope 后缀、是否允许复制）。

---

## 12. 下一步（建议）

1. 确认第 11 节 5 个问题，尤其是 IPv6 测试环境。
2. 抓 2–3 台真机的 `Probe`/`ProbeMatch` 原始报文（v4 与 v6 各一组），落成 `tests/fixtures/`。
3. 搭 M0/M1：先跑通编解码与 fixtures，再动 socket——协议层确定性最高，先锁死。
4. **并行启动 M2b 抓包预研**：这是唯一可能改变产品承诺的未知项，越早越好。
5. 用 `sadp-fake-device` 打通 CI 双栈端到端，再接真机。

---

## 13. 实现状态与偏差记录（2026-11-10）

### 13.1 已完成

| 模块 | 文件 | 说明 |
|---|---|---|
| 协议模型 | `crates/sadp-core/src/proto/model.rs` | 设备字段、MAC/IPv6 类型、去重键、逐字段合并 |
| 解码 | `crates/sadp-core/src/proto/parse.rs` | quick-xml 0.42 流式解析，容忍 BOM/注释/命名空间/未知字段/实体引用，字段与报文长度上限 |
| 编码 | `crates/sadp-core/src/proto/encode.rs` | `Probe` 生成，UUID 过滤防注入 |
| 传输层 | `crates/sadp-net/src/{iface,udp,mock}.rs` | 网卡聚合、双栈 socket、逐网卡组播出口、组播加入结果、MockTransport |
| 引擎 | `crates/sadp-core/src/scan/*` | Scanner 线程、Session 聚合、跨栈去重、老化、原始报文落盘 |
| 视图 | `crates/sadp-core/src/view.rs` | 排序（数值 IP 排序）、过滤、缺失值置底 |
| i18n | `crates/sadp-core/{src/i18n.rs,locales/*.yml}` | 中英全量 key，运行时切换 |
| CLI | `crates/sadp-cli/src/{main,args,output,commands,logging,clipboard}.rs` | 子命令、JSON/CSV/quiet、退出码、日志 |
| TUI | `crates/sadp-cli/src/tui.rs` | 表格/详情/过滤/排序/导出/复制/语言切换，渲染逻辑可离屏测试 |
| 假设备 | `crates/sadp-fake-device/` | 组播监听 + 单播应答，端到端测试用 |

测试：**102 个**（核心 56 + CLI 40 + 假设备 6，另有 1 个 `#[ignore]` 的组播端到端）。
`cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --check` 均通过。

### 13.2 与本文档原设计的偏差

1. **socket 模型：一族一个 socket，而非一网卡一个 socket**（§6.4）。
   绑到具体网卡地址的 socket 收不到广播与组播（Windows/POSIX 一致），
   而同一端口绑多个 `0.0.0.0` socket 会让单播回包归属不确定。
   改为「每族一个 socket + 发送前逐网卡 `IP_MULTICAST_IF`」，效果等价且更可靠；
   回包归属改为按源地址所在子网（v4）与 `scope_id`（v6）判定。
2. **新增 `--local-port`**：显式指定本地绑定端口，`0` 表示临时端口。
   既用于与官方 SADP 同机运行时避免抢 37020，也是同机假设备测试的前提。
3. **新增 `--stale <SECS>`**：把「老化」从内部概念变成可配置行为。
4. **i18n 取词改为 `tr()` / `tr_args()` 包装函数**，不使用 `t!` 宏直接跨 crate：
   `rust-i18n` 的 `t!` 展开为 `crate::_rust_i18n_t!`，跨 crate 调用无法解析；
   包装函数同时便于缺失 key 检测与 key 对齐测试。
5. **clap 用 builder API 而非 derive**：帮助文本需要运行期按 locale 注入，
   derive 的属性是编译期常量。
6. **配置文件暂缓**：语言优先级已实现 `--lang` > `SADP_LANG` > 系统区域 > `en-US`，
   但没有落盘配置；`--lang` / 环境变量已覆盖一期需求。
7. **`interfaces` 子命令额外输出每个网卡每个地址族的组播加入结果**（`v4✓ v6✗`），
   排障时不至于只有一句「失败」。
8. **`--repeat` 短于 `--timeout` 时自动收窄窗口**（并提示），而不是参数报错。
   `--repeat 1000` 配默认 3000 窗口是最自然的写法，直接拒绝属于实现洁癖。
9. **解析层必须拿到至少一个身份字段**（SN / MAC / IPv4 / 任意 IPv6），
   并且把 `0.0.0.0` 视为未配置。宽松 XML 解析会把端口上的杂散报文切成若干未知字段，
   原实现用「有未知字段也算设备」兜底，结果 TUI 里出现一排只有 `-` 的幽灵行。
   现在解析层与 `DeviceStore` 双重拦截：没有身份就不入库。
10. **语言切换同时接受 `L` 与 `l`**：不同终端对 Shift+L 的编码不一致
    （`Char('L')` 或 `Char('l')` + SHIFT），直接按 `l` 也很常见。
    正常模式没有文本输入，大小写都接受；过滤模式下 `l` 仍然只作为输入字符。
11. **暂停改为「停线程 / 起线程」**，不再用扫描器内部的 `AtomicBool` 暂停标志。
   原实现有两个状态源（UI 的 `paused` 字段与扫描器标志），且单轮扫描结束后
   空格会因「没有扫描器」直接返回，造成按键无反应、状态卡死；
   现在空格是纯粹的播放/暂停键：运行中则停，已停/已结束则起。

### 13.3 Gate 1 真机验证（Windows）

2026-11-13 在 Windows 11 专业工作站版（10.0.26300 / Ryzen 7 5800H）上，对现场球机
**DS-2DE4423DW-D/GLT/XM @ 192.168.1.120**（V7.3 build 241203，直连以太网）跑了清单：
**3 项通过、4 项部分、2 项因环境缺失未测**，明细见 [.gate1/gate1-report.md](../.gate1/gate1-report.md)。

- ✅ 多网卡/虚拟网卡隔离：以太网、WLAN、VMware×2、Tailscale、vgate0、回环逐卡扫描；
  未连接的 aTrust VPN 与 Not Present 的幽灵适配器在枚举阶段被过滤，不误报。
- ✅ JSON schema 冻结（`sadp.discover/v1` + 断言测试）、中英双语与 key 对齐。
- ⚠️ 真机矩阵只有球机 × Win11；IPv6-only 主机只有 v4 bind 失败的等价验证。
- ⚠️ 端口 37020 被占用时回退临时端口仍能发现设备；官方 SADP 未安装，属等效验证。
- ⛔ 真机双栈（设备不上报 IPv6）、防火墙（本机 mpssvc 未运行且无管理员权限）未测；
  24h 只做了 25 min 演练，脚本在 `.gate1/soak/`。
- 偏差：仓库未引入 `insta`，TUI/JSON 快照实为 `TestBackend` 文本断言与字段断言，
  明细见报告「6. JSON schema」。

真机测试还暴露并修复了 `--interface` 的匹配缺陷：纯数字既被当索引又被当 IPv4 前缀，
`-i 1` 会命中所有 `192.*` / `10.*` 网卡，回环扫描实际扫了全网卡。

### 13.4 二阶段（macOS）进展

已完成、可在任意平台验证的部分：

| 工作项 | 结果 |
|---|---|
| Apple target 交叉编译 | `cargo check --workspace --target {aarch64,x86_64}-apple-darwin` 均通过 |
| 平台差异收敛 | 无需平台专用 socket 代码：接收用通配地址，出口用 `IP_MULTICAST_IF`；`IP_BOUND_IF` 按设计不引入（理由见 `docs/MACOS.md`） |
| 权限诊断 | 新增「0 设备 + 0 个 SADP 响应报文」判定，macOS 上给出本地网络权限提示，其它平台给防火墙提示 |
| 通用二进制 | `scripts/build-macos-universal.sh`（双架构构建 + lipo + 可选签名/公证） |
| CI | `macos-15` / `macos-latest`（Apple Silicon）测试矩阵 + `x86_64-apple-darwin` 交叉检查；每个 OS 只构建一次 release 产物（Windows CLI+GUI、macOS 通用 CLI + GUI）、`cargo-deny` 依赖审计 |
| 文档 | `docs/MACOS.md`：权限、签名、公证、Gate 2 清单、故障排查 |
| 真实报文 | 现场设备（DS-2DE4423DW-D/GLT/XM）抓包脱敏为 `tests/fixtures/probe_match_real_ds2de.xml` |
| 型号展示 | 部分固件 `DeviceType` 是数字编码（196607），展示时回退到 `DeviceDescription`（表格/详情/导出），JSON 保留原始字段 |

macOS 真机验收（2026-11-12，macOS 26 / Apple Silicon）已完成 11/12 项：
本地网络授权弹窗两态、多网卡、端口占用回退、日志路径、导出/复制、链路本地 IPv6 的
scope 语义与 TUI 渲染均已实测通过，详见 [.gate2/gate2-report.md](../.gate2/gate2-report.md)。

仍未覆盖：24h 连续性（第 12 项，电池供电暂缓）、macOS 14/15 与 Intel 真机、macOS 应用防火墙行为。

### 13.5 三阶段（egui 图形界面）进展

新增 crate `crates/sadp-gui`（二进制 `sadp-discover-gui`），**不改核心**：GUI 只消费
`ScanEvent`、`DeviceStore`、`view::visible_indices` 与同一份 locale 文件。

| 里程碑 | 状态 | 说明 |
|---|---|---|
| G0 状态复用 | ✅ 已有基础 | 排序/过滤/可见行早在 `sadp_core::view`，TUI 与 GUI 共用；导出格式化逻辑在 GUI 侧独立实现（CLI 的 `output` 尚未抽公共层） |
| G1 壳与扫描 | ✅ | `eframe 0.37`（当前最新）窗口 + 扫描线程（`Scanner::poll(200ms)` → crossbeam 通道 → UI drain），暂停/继续 = 停线程/起线程 |
| G2 表格与详情 | ✅ 基础版 | `egui_extras::TableBuilder` 表格（IP/MAC/型号/来源/激活）、右侧详情、整行单击选中、双击 IP 用系统默认浏览器打开设备 Web 后台、过滤框、排序键与方向、状态栏与 toast |
| G3 设置与导出 | 🟡 部分 | 已有 JSON/CSV 导出（写当前目录）、剪贴板复制与日志目录跳转；网卡多选与文件对话框（`rfd`）未做，网卡固定用全部可用接口，地址族 / 端口等参数走命令行 |
| G4 体验 | 🟡 部分 | 中文字体已用 `fontdb` 从系统注入（macOS 取 PingFang SC 的 TTC face index），深色主题/字体回退顺序未细调 |
| G5 打包 | ⬜ 未开始 | `.app` 打包与签名公证、Windows MSI |

偏差记录：

1. **使用最新 eframe / egui 0.37**：0.37 把 `App::update(ctx, frame)` 换成了
   `App::ui(&mut Ui, frame)`，顶层面板由 `TopBottomPanel/SidePanel` 改为 `egui::Panel::{top,bottom,right}`
   （挂在根 `Ui` 上），`FullOutput.textures_delta` 必须显式消费（否则 Drop panic）。
   GUI 已按新 API 写好，后续跟随上游升级只需处理同类小改。
2. **日志模块与 CLI 重复**（`sadp-gui/src/logging.rs`）：抽公共 crate 前先复制，避免为一期功能动 CLI。
3. **导出无文件对话框**：直接写当前工作目录并提示路径，省一层依赖；代价是“另存为”体验缺失。
4. **GUI 可离屏测试**：`App::draw(&mut Ui)` 与 eframe 解耦，测试用 `Context::run_ui` + `MockTransport`
   + 真实报文 fixture 跑完整 UI pass，在没有窗口的 CI 里也能验证不 panic、事件确实进入 `DeviceStore`。
5. **Windows 单独用 glow 后端**：eframe 0.37 默认走 wgpu，而 `wgpu-hal 30.0.1` 的 D3D12 绑定与
   `windows 0.62.2` 不兼容（`fontique` 要求 ≥0.62.2，无法降级），Windows 上 `wgpu-hal` 直接编译失败
   （CI run 43245594522）。`sadp-gui` 因此按 target 拆分依赖：Windows 关默认特性、启用 glow；
   macOS/Linux 保持默认 wgpu。上游修好后删掉 Cargo.toml 里的 Windows 块即可。

测试：`sadp-gui` 13 个（导出 4 + Web 后台 URL 6 + 源码 i18n key 扫描 1 + 离屏渲染/过滤 2），全量 `cargo test --workspace` 通过（共 120 个）；
`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --check`、`cargo deny check` 通过。
覆盖率：`sadp-core` 行覆盖 **83.8%**、区域覆盖 82.8%（`cargo llvm-cov -p sadp-core`），满足 Gate 1 的 ≥80% 门槛。

### 13.6 建议的下一步

1. 抓 2–3 台真机的 `Probe`/`ProbeMatch`（v4 与 v6 各一组），替换/补充 `tests/fixtures/`。
2. 用 `--save-raw` 与 Wireshark 完成 M2b 的 IPv6 组地址确认，把结论回填到 §2.4。
3. Gate 1 剩余项：接入 IPC/NVR、Win10 主机、开启 IPv6 的真机补矩阵与双栈；
   管理员权限下验证防火墙首启；接电源跑 `.gate1/soak/start-24h.ps1`；
   macOS 14/15 真机跑 Gate 2 清单（`docs/MACOS.md` §5）。
4. GUI G3：网卡/地址族多选、`rfd` 另存为、`sadp-core` 导出格式化抽公共层供 CLI/GUI 共用。
