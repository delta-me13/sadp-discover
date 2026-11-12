//! i18n 一致性：中英 key 集合必须相同，占位符必须一致，且全部 key 都能取到值。
//!
//! 这组测试是 CI 的硬门槛（架构文档 Gate 1 第 7 条）。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sadp_core::i18n::{Lang, has_key, set};

static LOCK: Mutex<()> = Mutex::new(());

fn locales_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("locales")
}

fn flatten(prefix: &str, value: &serde_yaml::Value, out: &mut Vec<(String, String)>) {
    match value {
        serde_yaml::Value::Mapping(map) => {
            for (k, v) in map {
                let key = k.as_str().expect("yaml keys must be strings");
                let full = if prefix.is_empty() {
                    key.to_string()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten(&full, v, out);
            }
        }
        serde_yaml::Value::String(s) => out.push((prefix.to_string(), s.clone())),
        other => panic!("unexpected yaml value at {prefix}: {other:?}"),
    }
}

fn load(lang: &str) -> Vec<(String, String)> {
    let path: PathBuf = locales_dir().join(format!("{lang}.yml"));
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let value: serde_yaml::Value =
        serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    let mut out = Vec::new();
    flatten("", &value, &mut out);
    out
}

fn placeholders(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'%'
            && bytes[i + 1] == b'{'
            && let Some(end) = text[i + 2..].find('}')
        {
            out.insert(text[i + 2..i + 2 + end].to_string());
            i += 2 + end + 1;
            continue;
        }
        i += 1;
    }
    out
}

#[test]
fn key_sets_are_identical_in_both_languages() {
    let en = load("en-US");
    let zh = load("zh-CN");
    let en_keys: BTreeSet<_> = en.iter().map(|(k, _)| k.clone()).collect();
    let zh_keys: BTreeSet<_> = zh.iter().map(|(k, _)| k.clone()).collect();

    let missing_zh: Vec<_> = en_keys.difference(&zh_keys).cloned().collect();
    let extra_zh: Vec<_> = zh_keys.difference(&en_keys).cloned().collect();

    assert!(missing_zh.is_empty(), "zh-CN 缺少这些 key: {missing_zh:#?}");
    assert!(extra_zh.is_empty(), "zh-CN 多出这些 key: {extra_zh:#?}");
    assert!(en_keys.len() > 50, "key 数量异常: {}", en_keys.len());
}

#[test]
fn placeholders_match_between_languages() {
    let en = load("en-US");
    let zh = load("zh-CN");
    let zh_map: std::collections::BTreeMap<_, _> = zh.into_iter().collect();

    let mut problems = Vec::new();
    for (key, en_text) in &en {
        let Some(zh_text) = zh_map.get(key) else {
            continue;
        };
        let en_ph = placeholders(en_text);
        let zh_ph = placeholders(zh_text);
        if en_ph != zh_ph {
            problems.push(format!("{key}: en={en_ph:?} zh={zh_ph:?}"));
        }
    }
    assert!(
        problems.is_empty(),
        "占位符不一致:\n{}",
        problems.join("\n")
    );
}

#[test]
fn every_key_resolves_in_both_languages() {
    let _guard = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut expected = Vec::new();
    for lang in Lang::ALL {
        set(lang);
        for (key, value) in load(lang.code()) {
            assert!(has_key(&key), "{} 的 key `{key}` 取不到翻译", lang.code());
            let rendered = sadp_core::tr(&key);
            assert_eq!(
                rendered,
                value,
                "{} / {key} 渲染结果与 yaml 不一致",
                lang.code()
            );
            expected.push(key);
        }
    }
    assert!(!expected.is_empty());
    set(Lang::EnUs);
}

#[test]
fn no_hardcoded_ui_strings_in_core() {
    // 粗略守卫：core 的源码里不应出现成句中文（注释除外）。
    // 允许的行：注释、测试、断言消息、日志字段。
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();
    for entry in walk(&src) {
        // i18n.rs 里的语言自称（English / 简体中文）是语言学的「自名」，
        // 任何 locale 下都显示原文，不属于需要翻译的 UI 文案。
        if entry.file_name().is_some_and(|n| n == "i18n.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&entry) else {
            continue;
        };
        for (lineno, line) in text.lines().enumerate() {
            // 测试模块里的断言消息允许中文，不属于 UI 文案。
            if line.contains("#[cfg(test)]") {
                break;
            }
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!")
            {
                continue;
            }
            let in_string_zh = line
                .split('"')
                .nth(1)
                .is_some_and(|s| s.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
            if in_string_zh {
                offenders.push(format!(
                    "{}:{}: {}",
                    entry.display(),
                    lineno + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "UI 文案必须走 i18n key，发现硬编码中文:\n{}",
        offenders.join("\n")
    );
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(walk(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out
}
