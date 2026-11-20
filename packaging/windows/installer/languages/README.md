# Inno Setup 语言文件

`ChineseSimplified.isl` 来自 Inno Setup 官方源码仓库的 `is-6_7_4` 标签：

- 路径：`Files/Languages/Unofficial/ChineseSimplified.isl`
- 维护者：Zhenghan Yang（文件头有原始联系方式）
- 为什么随仓库带：Inno 6.7 的安装目录只有官方语言，简体中文 7.x 才进主干；
  直接引用 `compiler:Languages\ChineseSimplified.isl` 会在 CI 上编译失败。

升级 Inno Setup 大版本时，从对应的 `is-*` 标签重新取一份覆盖本文件，然后跑一次
`package (windows)` 的安装冒烟（`scripts/windows/smoke-install.ps1`）。`.isl` 里少了
消息会在编译期报错，不会静默回退成英文。

再分发遵循 Inno Setup License（允许随安装包分发翻译文件）。
