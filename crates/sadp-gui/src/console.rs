//! Windows：让「windows 子系统」的进程仍然能在命令行里正常说话。
//!
//! GUI 以 windows 子系统发布（双击不弹黑色控制台窗口），代价是进程启动时
//! 没有控制台，`--version`、`--help` 与日志都无处可去。从命令行启动时
//! 父进程其实是 cmd/PowerShell，它有自己的控制台——这里把它挂过来。

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
pub fn attach_parent_console() {
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_CHAR, FILE_TYPE_DISK,
        FILE_TYPE_PIPE, GetFileType, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE,
        SetStdHandle,
    };

    // SAFETY: 只是几个进程级的 Win32 调用，参数都是本函数内创建的合法值；
    // 句柄不关闭是刻意的——标准句柄要活到进程结束，由系统回收。
    unsafe {
        // 输出已被重定向（文件或管道）时不要抢回来：GUI 子系统进程照样继承
        // 父进程的句柄，`sadp-discover-gui --version > out.txt` 该写文件。
        // 句柄无效时 GetFileType 返回 FILE_TYPE_UNKNOWN，那才是需要挂控制台的情形。
        let kind = GetFileType(GetStdHandle(STD_OUTPUT_HANDLE));
        if matches!(kind, FILE_TYPE_CHAR | FILE_TYPE_DISK | FILE_TYPE_PIPE) {
            return;
        }

        // 父进程没有控制台（Explorer 双击）时返回 0，保持静默。
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return;
        }

        let name: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
        let handle = CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return;
        }
        SetStdHandle(STD_OUTPUT_HANDLE, handle);
        SetStdHandle(STD_ERROR_HANDLE, handle);
    }
}

/// 其它平台没有控制台挂载的概念：eframe 就是普通进程。
#[cfg(not(target_os = "windows"))]
pub fn attach_parent_console() {}
