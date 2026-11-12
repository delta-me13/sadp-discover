//! 剪贴板：调用系统自带工具，不引入额外依赖，也不破坏单文件分发。
//!
//! Windows 用 `clip`，macOS 用 `pbcopy`，Linux 用 `xclip`（不在排期内，但保留）。

use std::io::{self, Write};
use std::process::{Command, Stdio};

#[cfg(target_os = "windows")]
fn sink() -> Command {
    Command::new("clip")
}

#[cfg(target_os = "macos")]
fn sink() -> Command {
    Command::new("pbcopy")
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn sink() -> Command {
    let mut cmd = Command::new("xclip");
    cmd.arg("-selection").arg("clipboard");
    cmd
}

pub fn copy(text: &str) -> io::Result<()> {
    let mut child = sink()
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("clipboard process has no stdin"))?;
    stdin.write_all(text.as_bytes())?;
    // 必须显式关闭 stdin，否则 clip/pbcopy 会一直等 EOF。
    drop(stdin);

    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "clipboard tool exited with {status}"
        )))
    }
}
