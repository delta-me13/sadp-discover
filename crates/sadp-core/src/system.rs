//! 调用系统自带工具打开 URL 或目录。
//!
//! 平台差异只体现在「用哪个命令」：Windows 用 `rundll32` / `explorer`，
//! macOS 用 `open`，其它平台用 `xdg-open`。这里不参与扫描、协议或 UI 逻辑，
//! 只为 CLI 与 GUI 提供一个共用的「跳转到某处」入口。
//!
//! 只拉起进程，不等待退出，避免阻塞 TUI / UI 主循环。

use std::io;
use std::path::Path;
use std::process::Command;

/// 用系统默认浏览器打开 URL。
pub fn open_url(url: &str) -> io::Result<()> {
    launch(url_command(url))
}

/// 用系统文件管理器打开目录（不存在时由文件管理器自行报错）。
pub fn open_dir(path: &Path) -> io::Result<()> {
    launch(dir_command(path))
}

fn launch(mut command: Command) -> io::Result<()> {
    let mut child = command.spawn()?;
    // 后台回收，避免 Unix 上留下僵尸进程。
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(target_os = "windows")]
fn url_command(url: &str) -> Command {
    // 直接调 FileProtocolHandler，不经过 cmd，避免 URL 里的字符被 shell 解释。
    let mut cmd = Command::new("rundll32");
    cmd.arg("url.dll,FileProtocolHandler").arg(url);
    cmd
}

#[cfg(target_os = "macos")]
fn url_command(url: &str) -> Command {
    let mut cmd = Command::new("open");
    cmd.arg(url);
    cmd
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn url_command(url: &str) -> Command {
    let mut cmd = Command::new("xdg-open");
    cmd.arg(url);
    cmd
}

#[cfg(target_os = "windows")]
fn dir_command(path: &Path) -> Command {
    // explorer 打开目录的退出码不可靠（成功也可能是 1），因此只关心能否成功拉起。
    let mut cmd = Command::new("explorer");
    cmd.arg(path);
    cmd
}

#[cfg(target_os = "macos")]
fn dir_command(path: &Path) -> Command {
    let mut cmd = Command::new("open");
    cmd.arg(path);
    cmd
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn dir_command(path: &Path) -> Command {
    let mut cmd = Command::new("xdg-open");
    cmd.arg(path);
    cmd
}
