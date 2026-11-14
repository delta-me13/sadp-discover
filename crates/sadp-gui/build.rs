//! 把图标、应用清单与版本信息嵌进 sadp-discover-gui.exe。
//!
//! 逻辑在 packaging/windows/build-resources.rs（CLI 共用），这里只声明「我叫什么」。

include!("../../packaging/windows/build-resources.rs");

fn main() {
    embed_resources(
        "sadp-discover-gui",
        "sadp-discover-gui.exe",
        "SADP device discovery (GUI)",
    );
}
