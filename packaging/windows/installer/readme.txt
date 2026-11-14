sadp-discover：只读的海康（Hikvision）SADP 设备发现工具

安装位置
  %LOCALAPPDATA%\Programs\sadp-discover
    sadp-discover-gui.exe   图形界面（开始菜单里的快捷方式指向它）
    sadp-discover.exe       命令行与 TUI
    README.md、LICENSE-*    说明与许可证

  勾选了「把 sadp-discover 加入 PATH」的话，可以直接在命令行里跑
  sadp-discover --help。新开的命令行窗口才会生效。

首次运行
  第一次扫描时 Windows 防火墙会弹窗询问是否允许访问网络，请勾选「专用网络」
  并允许。拒绝之后设备发的回包会被丢掉，界面上就是「一台设备都没有」。
  没有弹窗或误点拒绝时，用管理员命令行补一条规则：
    netsh advfirewall firewall add rule name="sadp-discover" dir=in action=allow ^
      program="%LOCALAPPDATA%\Programs\sadp-discover\sadp-discover-gui.exe" profile=private

卸载
  「设置 → 应用 → 已安装的应用」里找到 sadp-discover 卸载，
  或运行安装目录下的 unins000.exe。PATH 里的条目会一并删除。
