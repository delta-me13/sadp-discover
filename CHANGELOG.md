# Changelog

格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循语义化版本。

> 发布状态：`v0.1.0` 的 Git tag 与 GitHub Release 尚未创建，下方对比/发布链接在补 tag 前不可用。

## [Unreleased]

### Added

- 三阶段图形界面：新增 `sadp-gui` crate（二进制 `sadp-discover-gui`），基于最新的 eframe / egui 0.37
  - 表格（IP / MAC / 型号 / 来源 / 激活）、详情面板、行选中、过滤、排序键与方向
  - 单击整行即可选中并查看详情；双击 IP 用系统默认浏览器打开设备 Web 后台（IPv4 优先，IPv6 自动加方括号与 scope，端口 443 自动用 HTTPS）
  - JSON / CSV 导出、剪贴板复制、中英切换，复用与 TUI 相同的扫描引擎与 locale
  - 中文字体从系统注入（fontdb，macOS 取 PingFang SC 的 TTC face index）
- `logs` 子命令 / GUI 状态栏「日志」按钮 / TUI `J` 键：一键打开日志文件夹
- `Transport::sent_packets()`：累计成功发出的数据报数，用于区分「发不出去」与「没人回」
- 24h 连续性压测脚本（`.gate2/soak/`）：电源门禁、caffeinate、采样与自动裁决
- `LICENSE-MIT` / `LICENSE-APACHE`、`CHANGELOG.md`、`deny.toml`（依赖许可 / 漏洞 / 来源审计）
- CI：`macos-15` / `macos-latest`(26) 双版本矩阵，按 OS 只构建一次 release 产物，`cargo-deny` job

### Fixed

- GUI 表格「激活」列与详情「DHCP」行把不存在的 `tui.yes` / `tui.no` / `tui.none` 当成 key，界面上直接显示 key 原文；改用 `common.*`，并新增源码 key 扫描测试防止同类漏配
- GUI 表格文字默认参与文本选择，点中 IP/型号等文字会吞掉点击，导致整行选中与双击 IP 只有空白处生效；表格区域关闭 `selectable_labels`，点击可穿透到整行
- macOS 上「0 设备」不再固定误报本地网络权限：只有探测包发不出去时才提示权限（Gate 2 观察项 F2）
- Windows 上 GUI 无法构建：`wgpu-hal 30` 的 D3D12 绑定与 `windows 0.62.2` 不兼容，Windows 改用 glow 后端

### Changed

- README 状态区简化为能力概览，里程碑细节归档到 `docs/ARCHITECTURE.md`

## [0.1.0] - 2026-11-12

### Added

- SADP 协议编解码：宽松 XML 解析（BOM / 注释 / 命名空间 / 实体 / 老固件嵌套字段）
- 双栈发现：IPv4 组播 + 逐网卡子网广播、IPv6 链路本地组播；跨栈按 SN → MAC → IP 去重合并
- TUI：表格、详情、过滤、排序、导出、复制、语言切换（80×24 可用）
- CLI：`scan` / `interfaces` / `completions` 子命令，`--json`（schema `sadp.discover/v1`）/ `--csv` / `--quiet`，约定退出码
- 中英双语：CLI 帮助、TUI、错误提示，含 key 对齐测试
- macOS 适配：通用二进制构建脚本、签名/公证说明、本地网络权限与防火墙排障文档
- `sadp-fake-device`：无真机时的端到端假设备

[Unreleased]: https://github.com/delta-me13/sadp-discover/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/delta-me13/sadp-discover/releases/tag/v0.1.0
