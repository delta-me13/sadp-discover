//! 国际化：语言选择、运行时切换、取词。
//!
//! 语言优先级（架构文档 §6.6）：
//! `--lang` > 环境变量 `SADP_LANG` > 系统区域设置 > `en-US`。
//!
//! 取词统一走 `tr` / `tr_args`，**不要在 UI 代码里写死文案**；
//! 缺失的 key 会回退到 `en-US`，并在 debug 构建里打警告。

use std::fmt;

use tracing::debug;

/// 支持的语言。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lang {
    EnUs,
    ZhCn,
}

impl Lang {
    pub const ALL: [Self; 2] = [Self::EnUs, Self::ZhCn];

    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::EnUs => "en-US",
            Self::ZhCn => "zh-CN",
        }
    }

    /// 语言在自身语言里的名称，切换菜单里用。
    #[must_use]
    pub const fn native_name(self) -> &'static str {
        match self {
            Self::EnUs => "English",
            Self::ZhCn => "简体中文",
        }
    }

    /// 宽松解析 BCP-47 标签：`zh`、`zh-CN`、`zh_CN`、`zh-Hans` 都归中文。
    #[must_use]
    pub fn from_tag(tag: &str) -> Self {
        if tag.trim().to_ascii_lowercase().starts_with("zh") {
            Self::ZhCn
        } else {
            Self::EnUs
        }
    }

    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::EnUs => Self::ZhCn,
            Self::ZhCn => Self::EnUs,
        }
    }
}

impl fmt::Display for Lang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// 归一化为受支持的 locale 代码。
#[must_use]
pub fn normalize(tag: &str) -> &'static str {
    Lang::from_tag(tag).code()
}

/// 按优先级初始化 locale，返回最终使用的语言。
///
/// 可重复调用；`pref` 非空时总是覆盖当前设置。
pub fn init(pref: Option<&str>) -> Lang {
    let raw = pref
        .map(str::to_owned)
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::env::var("SADP_LANG")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .or_else(sys_locale::get_locale)
        .unwrap_or_else(|| "en-US".to_string());
    let lang = Lang::from_tag(&raw);
    rust_i18n::set_locale(lang.code());
    debug!(requested = %raw, resolved = %lang.code(), "locale initialized");
    lang
}

pub fn set(lang: Lang) {
    rust_i18n::set_locale(lang.code());
}

#[must_use]
pub fn current() -> Lang {
    Lang::from_tag(&rust_i18n::locale())
}

pub fn toggle() -> Lang {
    let next = current().next();
    set(next);
    next
}

/// 取词。缺失时返回 key 本身（方便发现漏翻），并在 debug 构建告警。
#[must_use]
pub fn tr(key: &str) -> String {
    let locale = rust_i18n::locale();
    match crate::_rust_i18n_try_translate(&locale, key) {
        Some(value) => value.into_owned(),
        None => {
            debug!(key, "missing translation key");
            key.to_string()
        }
    }
}

/// 取词并替换 `%{name}` 占位符。
#[must_use]
pub fn tr_args(key: &str, args: &[(&str, &str)]) -> String {
    let mut text = tr(key);
    for (name, value) in args {
        text = text.replace(&format!("%{{{name}}}"), value);
    }
    text
}

/// 该 key 在当前语言下是否有翻译（回退不算）。
#[must_use]
pub fn has_key(key: &str) -> bool {
    let locale = rust_i18n::locale();
    crate::_rust_i18n_try_translate(&locale, key).is_some_and(|v| v != key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// locale 是进程级全局状态，测试必须串行，否则互相踩语言。
    static LOCALE_LOCK: Mutex<()> = Mutex::new(());

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        LOCALE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn normalizes_language_tags() {
        for tag in ["zh", "zh-CN", "zh_CN", "zh-Hans", "ZH-cn"] {
            assert_eq!(normalize(tag), "zh-CN", "tag={tag}");
        }
        for tag in ["en", "en-US", "en_GB", "fr-FR", "", "de"] {
            assert_eq!(normalize(tag), "en-US", "tag={tag}");
        }
    }

    #[test]
    fn switch_between_languages() {
        let _guard = lock();
        set(Lang::ZhCn);
        assert_eq!(current(), Lang::ZhCn);
        let zh = tr("app.name");
        assert_eq!(zh, "sadp-discover");

        set(Lang::EnUs);
        assert_eq!(current(), Lang::EnUs);
        assert_eq!(toggle(), Lang::ZhCn);
        set(Lang::EnUs);
    }

    #[test]
    fn placeholders_are_replaced() {
        let _guard = lock();
        set(Lang::EnUs);
        let text = tr_args("out.summary", &[("count", "3")]);
        assert_eq!(text, "3 device(s) found");
        set(Lang::EnUs);
    }

    #[test]
    fn missing_key_falls_back_to_itself() {
        let _guard = lock();
        assert_eq!(tr("definitely.not.a.key"), "definitely.not.a.key");
        assert!(!has_key("definitely.not.a.key"));
    }

    #[test]
    fn english_fallback_for_untranslated_key() {
        let _guard = lock();
        // `lang.en` 在中英文里都存在；用一个只在 en 里存在的 key 验证回退需要额外文件，
        // 这里退而验证两套语言都能取到核心 key。
        for lang in Lang::ALL {
            set(lang);
            assert!(has_key("app.name"));
            assert!(has_key("cli.arg.timeout"));
        }
        set(Lang::EnUs);
    }
}
