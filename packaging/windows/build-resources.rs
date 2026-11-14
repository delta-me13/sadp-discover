// 由 crates/sadp-cli/build.rs 与 crates/sadp-gui/build.rs 通过 include! 引入，
// 两个 exe 的 Windows 资源逻辑完全相同，只有名字和描述不一样。
//
// 目标平台不是 Windows 时整段跳过：本地 macOS/Linux 构建、以及
// `cargo check --target x86_64-pc-windows-msvc` 的交叉检查都不需要 rc.exe。

use std::path::PathBuf;

fn embed_resources(internal_name: &str, original_filename: &str, description: &str) {
    // 读的是目标三元组（不是主机），macOS 上构建时这里就返回了。
    if std::env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }

    let crate_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let root = crate_dir.join("..").join("..");
    let template = root.join("packaging/windows/app.rc.in");
    let helper = root.join("packaging/windows/build-resources.rs");
    let icon = root.join("assets/sadp-discover.ico");
    let manifest = root.join("packaging/windows/app.manifest");

    // 一旦输出 rerun-if-changed，cargo 就不再默认跟踪整个 crate 目录，
    // 所以模板、图标、清单、以及这个文件本身都要显式列出来。
    for path in [&template, &helper, &icon, &manifest] {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    // 主机不是 Windows 时没有 rc.exe：交叉 check 只需要验证代码能编译，
    // 资源交给 CI 的 windows runner 生成。
    if !cfg!(windows) {
        println!("cargo:warning=非 Windows 主机，跳过资源嵌入（交叉检查）");
        return;
    }

    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let rc = std::fs::read_to_string(&template)
        .expect("读取 packaging/windows/app.rc.in")
        .replace("@ICON@", &absolute(&icon))
        .replace("@MANIFEST@", &absolute(&manifest))
        .replace("@VER_NUM@", &version_numbers(&version))
        .replace("@VERSION@", &version)
        .replace("@DESCRIPTION@", description)
        .replace("@INTERNAL_NAME@", internal_name)
        .replace("@ORIGINAL_FILENAME@", original_filename);

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let rc_path = out.join("sadp-discover.rc");
    std::fs::write(&rc_path, rc).expect("写 OUT_DIR/sadp-discover.rc");

    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_optional()
        .expect("rc.exe 编译资源失败");
}

/// rc.exe 按自己的当前目录解析相对路径，而生成的 .rc 在 OUT_DIR 里，
/// 所以图标和清单一律换成绝对路径。
///
/// 分隔符必须是正斜杠：rc.exe 把带引号的资源文件名当 C 字符串过一遍转义，
/// `D:\a\...` 里的 `\a` 会被当成响铃（BEL），路径就变成 `D:` + BEL + `...`，
/// 报 `RC2135: file not found`。Windows API 一样认正斜杠。
fn absolute(path: &std::path::Path) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
        .replace('\\', "/")
}

/// "0.1.0" / "0.1.0-rc.1" → "0,1,0"：VERSIONINFO 只收三个数字段。
fn version_numbers(version: &str) -> String {
    let mut parts: Vec<String> = version
        .split('.')
        .take(3)
        .map(|part| {
            part.split('-')
                .next()
                .unwrap_or("0")
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .collect();
    while parts.len() < 3 {
        parts.push("0".into());
    }
    for part in &mut parts {
        if part.is_empty() {
            *part = "0".into();
        }
    }
    parts.join(",")
}
