//! 让 cargo 跟踪 `locales/` 的变更。
//!
//! `rust-i18n` 的 `i18n!` 宏在**编译期**读取 YAML 并把内容内联进二进制，
//! 但不会向 cargo 声明文件依赖。结果就是改了翻译后 `cargo test` 仍然用旧文案，
//! 直到某个不相关的源文件被改动。这里显式声明依赖，避免这种「假通过」。

fn main() {
    println!("cargo:rerun-if-changed=locales");
    if let Ok(entries) = std::fs::read_dir("locales") {
        for entry in entries.flatten() {
            println!("cargo:rerun-if-changed={}", entry.path().display());
        }
    }
}
