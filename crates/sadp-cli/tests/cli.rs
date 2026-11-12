//! CLI 黑盒测试：退出码、双语帮助、JSON schema、子命令。

use std::process::Command;

use assert_cmd::prelude::*;
use predicates::prelude::*;

fn bin() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin("sadp-discover"))
}

#[test]
fn version_is_printed() {
    bin()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("sadp-discover"));
}

#[test]
fn help_is_english_by_default() {
    bin()
        .args(["--lang", "en-US", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Discover Hikvision devices"))
        .stdout(predicate::str::contains("EXIT CODES"));
}

#[test]
fn help_is_chinese_with_lang_flag() {
    // -h 展示短描述，--help 展示长描述与页脚，两者都必须本地化。
    bin()
        .args(["--lang", "zh-CN", "-h"])
        .assert()
        .success()
        .stdout(predicate::str::contains("发现局域网内的海康"));

    bin()
        .args(["--lang", "zh-CN", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("退出码"))
        .stdout(predicate::str::contains("发现局域网内支持 SADP 协议"));
}

#[test]
fn subcommand_help_is_localized_too() {
    bin()
        .args(["--lang", "zh-CN", "interfaces", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("列出网卡"));
}

#[test]
fn invalid_timeout_exits_with_code_two() {
    bin()
        .args(["--timeout", "abc", "--no-tui"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("无效").or(predicate::str::contains("invalid")));
}

#[test]
fn unparseable_port_exits_with_code_two() {
    bin().args(["--port", "70000", "--no-tui"]).assert().code(2);
}

#[test]
fn json_and_csv_conflict_exits_with_code_two() {
    bin().args(["--json", "--csv", "--no-tui"]).assert().code(2);
}

#[test]
fn interfaces_json_reports_schema() {
    bin()
        .args(["interfaces", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"schema\": \"sadp.interfaces/v1\"",
        ))
        .stdout(predicate::str::contains("\"interfaces\""));
}

#[test]
fn completions_are_generated() {
    bin()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("sadp-discover"));
}

#[test]
fn json_scan_output_matches_frozen_schema() {
    let output = bin()
        .args([
            "--no-tui",
            "--json",
            "--timeout",
            "120",
            "--local-port",
            "0",
            "--family",
            "v4",
        ])
        .output()
        .expect("run scanner");

    // 0 = 发现设备，1 = 未发现；两种都算正常。
    let code = output.status.code().expect("exit code");
    assert!(code == 0 || code == 1, "unexpected exit code {code}");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("valid json");
    assert_eq!(value["schema"], "sadp.discover/v1");
    assert_eq!(value["tool"], "sadp-discover");
    assert!(value["devices"].is_array());
    assert!(value["port"].is_number());
    assert!(value["generated_at"].is_string());
}

#[test]
fn unknown_interface_exits_with_code_two() {
    bin()
        .args([
            "--no-tui",
            "--json",
            "--timeout",
            "100",
            "--interface",
            "definitely-not-an-interface-xyz",
        ])
        .assert()
        .code(2);
}

#[test]
fn quiet_mode_prints_addresses_only() {
    let output = bin()
        .args([
            "--no-tui",
            "--quiet",
            "--timeout",
            "100",
            "--local-port",
            "0",
            "--family",
            "v4",
        ])
        .output()
        .expect("run scanner");
    let code = output.status.code().expect("exit code");
    assert!(code == 0 || code == 1);
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        assert!(
            line.parse::<std::net::IpAddr>().is_ok(),
            "quiet 输出只允许地址，实际: {line:?}"
        );
    }
}
