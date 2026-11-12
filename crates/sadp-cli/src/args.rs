//! 命令行参数定义与解析。
//!
//! 全部 help 文案在构建 `Command` 时通过 `tr()` 注入，因此 `--lang` 必须**先于**
//! clap 解析被处理（`pre_scan_lang`），这是 CLI 双语的关键（架构文档 §6.6）。

use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;
use std::time::Duration;

use clap::error::ErrorKind;
use clap::{Arg, ArgAction, ArgMatches, Command as ClapCommand};
use sadp_core::Family;
use sadp_core::i18n::{tr, tr_args};

pub const DEFAULT_TIMEOUT_MS: u64 = 3000;

#[derive(Debug)]
pub struct Cli {
    pub kind: Kind,
    pub lang: Option<String>,
    pub log_level: Option<String>,
    pub verbose: u8,
}

#[derive(Debug)]
pub enum Kind {
    Scan(ScanArgs),
    Interfaces(InterfacesArgs),
    Completions(CompletionsArgs),
}

#[derive(Clone, Debug)]
pub struct ScanArgs {
    pub timeout: Duration,
    pub repeat: Option<Duration>,
    pub port: u16,
    pub local_port: Option<u16>,
    pub interfaces: Vec<String>,
    pub bind: Option<Ipv4Addr>,
    pub families: Vec<Family>,
    pub v6_mcast: Ipv6Addr,
    pub passive: bool,
    pub json: bool,
    pub csv: bool,
    pub quiet: bool,
    pub no_tui: bool,
    pub output: Option<PathBuf>,
    pub save_raw: Option<PathBuf>,
    pub stale: Option<Duration>,
}

#[derive(Clone, Debug, Default)]
pub struct InterfacesArgs {
    pub json: bool,
}

#[derive(Clone, Debug)]
pub struct CompletionsArgs {
    pub shell: clap_complete::Shell,
}

impl ScanArgs {
    /// 是否走交互界面。
    #[must_use]
    pub fn wants_tui(&self) -> bool {
        !self.no_tui && !self.json && !self.csv && !self.quiet && self.output.is_none()
    }

    /// 实际使用的单轮窗口。
    ///
    /// `--repeat` 是「每 N 毫秒开始新一轮」的周期；若它比 `--timeout` 还短，
    /// 就把窗口收窄到周期，而不是直接报错——`--repeat 1000` 配默认 3000 窗口
    /// 是很自然的写法，不应该拒绝。
    #[must_use]
    pub fn effective_timeout(&self) -> Duration {
        match self.repeat {
            Some(repeat) if repeat < self.timeout => repeat,
            _ => self.timeout,
        }
    }

    /// 需要提示用户但不致命的调整。
    #[must_use]
    pub fn notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        if let Some(repeat) = self.repeat
            && repeat < self.timeout
        {
            notes.push(tr_args(
                "cli.note.window_clamped",
                &[
                    ("timeout", &self.timeout.as_millis().to_string()),
                    ("repeat", &repeat.as_millis().to_string()),
                    ("used", &self.effective_timeout().as_millis().to_string()),
                ],
            ));
        }
        notes
    }

    pub fn scan_config(&self) -> Result<sadp_core::ScanConfig, sadp_core::ConfigError> {
        let cfg = sadp_core::ScanConfig {
            port: self.port,
            timeout: self.effective_timeout(),
            repeat: self.repeat,
            families: self.families.clone(),
            v6_mcast: self.v6_mcast,
            broadcast: true,
            interfaces: sadp_net::iface::InterfaceSelector::only(self.interfaces.clone()),
            passive: self.passive,
            stale_after: self.stale,
            save_raw: self.save_raw.clone(),
        };
        cfg.validate()?;
        Ok(cfg)
    }
}

/// 预扫描 `--lang`：此时 clap 还没解析，帮助文本却已经需要语言。
#[must_use]
pub fn pre_scan_lang<I: Iterator<Item = String>>(args: I) -> Option<String> {
    let mut it = args;
    while let Some(arg) = it.next() {
        if arg == "--lang" {
            return it.next();
        }
        if let Some(value) = arg.strip_prefix("--lang=")
            && !value.is_empty()
        {
            return Some(value.to_string());
        }
    }
    None
}

/// 构建 clap 命令（每次调用都重新取文案，便于语言切换后重建）。
#[must_use]
pub fn command() -> ClapCommand {
    ClapCommand::new("sadp-discover")
        .version(env!("CARGO_PKG_VERSION"))
        .about(tr("app.about"))
        .long_about(tr("app.long_about"))
        .after_help(tr("cli.after_help"))
        .disable_help_subcommand(true)
        .subcommand_required(false)
        .arg_required_else_help(false)
        .arg(lang_arg())
        .arg(log_level_arg())
        .arg(verbose_arg())
        .args(scan_option_args())
        .subcommand(
            ClapCommand::new("scan")
                .about(tr("cli.cmd.scan"))
                .args(scan_option_args()),
        )
        .subcommand(
            ClapCommand::new("interfaces")
                .about(tr("cli.cmd.interfaces"))
                .arg(
                    Arg::new("json")
                        .long("json")
                        .action(ArgAction::SetTrue)
                        .help(tr("cli.arg.json")),
                ),
        )
        .subcommand(
            ClapCommand::new("completions")
                .about(tr("cli.cmd.completions"))
                .arg(
                    Arg::new("shell")
                        .value_name("SHELL")
                        .required(true)
                        .value_parser(["bash", "zsh", "fish", "powershell", "elvish"])
                        .help(tr("cli.arg.shell")),
                ),
        )
}

fn lang_arg() -> Arg {
    Arg::new("lang")
        .long("lang")
        .value_name("LANG")
        .global(true)
        .value_parser(["en-US", "zh-CN", "en", "zh"])
        .help(tr("cli.arg.lang"))
}

fn log_level_arg() -> Arg {
    Arg::new("log_level")
        .long("log-level")
        .value_name("LEVEL")
        .global(true)
        .value_parser(["error", "warn", "info", "debug", "trace"])
        .help(tr("cli.arg.log_level"))
}

fn verbose_arg() -> Arg {
    Arg::new("verbose")
        .short('v')
        .long("verbose")
        .action(ArgAction::Count)
        .global(true)
        .help(tr("cli.arg.verbose"))
}

fn scan_option_args() -> Vec<Arg> {
    vec![
        Arg::new("timeout")
            .long("timeout")
            .value_name("MS")
            .default_value("3000")
            .help(tr_args(
                "cli.arg.timeout",
                &[("default", &DEFAULT_TIMEOUT_MS.to_string())],
            )),
        Arg::new("repeat")
            .long("repeat")
            .value_name("MS")
            .help(tr("cli.arg.repeat")),
        Arg::new("local-port")
            .long("local-port")
            .value_name("PORT")
            .help(tr("cli.arg.local_port")),
        Arg::new("port")
            .long("port")
            .value_name("PORT")
            .default_value("37020")
            .help(tr_args("cli.arg.port", &[("default", "37020")])),
        Arg::new("interface")
            .long("interface")
            .short('i')
            .value_name("IFACE")
            .action(ArgAction::Append)
            .value_delimiter(',')
            .help(tr("cli.arg.interface")),
        Arg::new("bind")
            .long("bind")
            .value_name("ADDR")
            .help(tr("cli.arg.bind")),
        Arg::new("family")
            .long("family")
            .value_name("FAMILY")
            .default_value("both")
            .value_parser(["v4", "v6", "both"])
            .help(tr("cli.arg.family")),
        Arg::new("v6-mcast")
            .long("v6-mcast")
            .value_name("ADDR")
            .default_value("ff02::1")
            .help(tr_args("cli.arg.v6_mcast", &[("default", "ff02::1")])),
        Arg::new("passive")
            .long("passive")
            .action(ArgAction::SetTrue)
            .help(tr("cli.arg.passive")),
        Arg::new("json")
            .long("json")
            .action(ArgAction::SetTrue)
            .help(tr("cli.arg.json")),
        Arg::new("csv")
            .long("csv")
            .action(ArgAction::SetTrue)
            .help(tr("cli.arg.csv")),
        Arg::new("quiet")
            .long("quiet")
            .short('q')
            .action(ArgAction::SetTrue)
            .help(tr("cli.arg.quiet")),
        Arg::new("no-tui")
            .long("no-tui")
            .action(ArgAction::SetTrue)
            .help(tr("cli.arg.no_tui")),
        Arg::new("output")
            .long("output")
            .short('o')
            .value_name("PATH")
            .help(tr("cli.arg.output")),
        Arg::new("save-raw")
            .long("save-raw")
            .value_name("DIR")
            .help(tr("cli.arg.save_raw")),
        Arg::new("stale")
            .long("stale")
            .value_name("SECS")
            .help(tr("cli.arg.stale")),
    ]
}

/// 解析进程参数。用法错误以 `clap::Error` 返回，由调用方统一打印并映射退出码。
pub fn parse() -> Result<Cli, clap::Error> {
    let matches = command().get_matches();
    let lang = global_string(&matches, "lang");
    let log_level = global_string(&matches, "log_level");
    let verbose = global_count(&matches, "verbose");

    let kind = match matches.subcommand() {
        Some(("scan", sub)) => Kind::Scan(ScanArgs::from_matches(sub)?),
        Some(("interfaces", sub)) => Kind::Interfaces(InterfacesArgs {
            json: sub.get_flag("json"),
        }),
        Some(("completions", sub)) => {
            let shell = sub
                .get_one::<String>("shell")
                .expect("required by clap")
                .parse::<clap_complete::Shell>()
                .map_err(|e| clap_error(ErrorKind::InvalidValue, e.to_string()))?;
            Kind::Completions(CompletionsArgs { shell })
        }
        None => Kind::Scan(ScanArgs::from_matches(&matches)?),
        Some((name, _)) => {
            return Err(clap_error(
                ErrorKind::InvalidSubcommand,
                format!("unknown subcommand `{name}`"),
            ));
        }
    };

    Ok(Cli {
        kind,
        lang,
        log_level,
        verbose,
    })
}

fn global_string(matches: &ArgMatches, name: &str) -> Option<String> {
    if let Some((_, sub)) = matches.subcommand()
        && let Some(v) = sub.get_one::<String>(name)
    {
        return Some(v.clone());
    }
    matches.get_one::<String>(name).cloned()
}

fn global_count(matches: &ArgMatches, name: &str) -> u8 {
    let root = matches.get_count(name);
    if root > 0 {
        return root;
    }
    matches
        .subcommand()
        .map_or(0, |(_, sub)| sub.get_count(name))
}

impl ScanArgs {
    fn from_matches(m: &ArgMatches) -> Result<Self, clap::Error> {
        let timeout_ms = parse_u64(m, "timeout", "cli.error.bad_timeout")?;
        if timeout_ms == 0 {
            return Err(clap_error(
                ErrorKind::InvalidValue,
                tr_args("cli.error.bad_timeout", &[("value", "0")]),
            ));
        }
        let timeout = Duration::from_millis(timeout_ms);

        let repeat = match m.get_one::<String>("repeat") {
            Some(raw) => {
                let ms = raw.parse::<u64>().map_err(|_| {
                    clap_error(
                        ErrorKind::InvalidValue,
                        tr_args("cli.error.bad_repeat", &[("value", raw)]),
                    )
                })?;
                if ms == 0 {
                    return Err(clap_error(
                        ErrorKind::InvalidValue,
                        tr_args("cli.error.bad_repeat", &[("value", "0")]),
                    ));
                }
                Some(Duration::from_millis(ms))
            }
            None => None,
        };

        let port = parse_u16(m, "port", "cli.error.bad_port")?;
        if port == 0 {
            return Err(clap_error(
                ErrorKind::InvalidValue,
                tr_args("cli.error.bad_port", &[("value", "0")]),
            ));
        }

        let families = match m.get_one::<String>("family").map(String::as_str) {
            Some("v4") => vec![Family::V4],
            Some("v6") => vec![Family::V6],
            _ => vec![Family::V4, Family::V6],
        };

        let bind = match m.get_one::<String>("bind") {
            Some(raw) => Some(raw.trim().parse::<Ipv4Addr>().map_err(|_| {
                clap_error(
                    ErrorKind::InvalidValue,
                    format!("invalid IPv4 bind address `{raw}`"),
                )
            })?),
            None => None,
        };

        let v6_mcast = m
            .get_one::<String>("v6-mcast")
            .map(String::as_str)
            .unwrap_or("ff02::1")
            .trim()
            .parse::<Ipv6Addr>()
            .map_err(|_| {
                clap_error(
                    ErrorKind::InvalidValue,
                    "invalid IPv6 multicast address".to_string(),
                )
            })?;
        let stale = match m.get_one::<String>("stale") {
            Some(raw) => {
                let secs = raw.parse::<u64>().map_err(|_| {
                    clap_error(
                        ErrorKind::InvalidValue,
                        tr_args("cli.error.bad_stale", &[("value", raw)]),
                    )
                })?;
                if secs == 0 {
                    None
                } else {
                    Some(Duration::from_secs(secs))
                }
            }
            None => None,
        };

        let json = m.get_flag("json");
        let csv = m.get_flag("csv");
        if json && csv {
            return Err(clap_error(
                ErrorKind::ArgumentConflict,
                "--json and --csv cannot be used together".to_string(),
            ));
        }

        let interfaces = m
            .get_many::<String>("interface")
            .map(|vals| {
                vals.map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let local_port = match m.get_one::<String>("local-port") {
            Some(raw) => Some(raw.trim().parse::<u16>().map_err(|_| {
                clap_error(
                    ErrorKind::InvalidValue,
                    tr_args("cli.error.bad_port", &[("value", raw)]),
                )
            })?),
            None => None,
        };

        Ok(Self {
            timeout,
            repeat,
            port,
            local_port,
            interfaces,
            bind,
            families,
            v6_mcast,
            passive: m.get_flag("passive"),
            json,
            csv,
            quiet: m.get_flag("quiet"),
            no_tui: m.get_flag("no-tui"),
            output: m.get_one::<String>("output").map(PathBuf::from),
            save_raw: m.get_one::<String>("save-raw").map(PathBuf::from),
            stale,
        })
    }
}

fn parse_u64(m: &ArgMatches, name: &str, key: &str) -> Result<u64, clap::Error> {
    let raw = m.get_one::<String>(name).map_or("", String::as_str);
    raw.trim()
        .parse::<u64>()
        .map_err(|_| clap_error(ErrorKind::InvalidValue, tr_args(key, &[("value", raw)])))
}

fn parse_u16(m: &ArgMatches, name: &str, key: &str) -> Result<u16, clap::Error> {
    let raw = m.get_one::<String>(name).map_or("", String::as_str);
    raw.trim()
        .parse::<u16>()
        .map_err(|_| clap_error(ErrorKind::InvalidValue, tr_args(key, &[("value", raw)])))
}

fn clap_error(kind: ErrorKind, message: String) -> clap::Error {
    clap::Error::raw(kind, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_scan_finds_lang_in_both_forms() {
        let args = vec![
            "sadp-discover".to_string(),
            "--lang".to_string(),
            "zh-CN".to_string(),
        ];
        assert_eq!(pre_scan_lang(args.into_iter()), Some("zh-CN".into()));

        let args = vec!["sadp-discover".to_string(), "--lang=en-US".to_string()];
        assert_eq!(pre_scan_lang(args.into_iter()), Some("en-US".into()));

        let args = vec![
            "sadp-discover".to_string(),
            "--timeout".to_string(),
            "100".to_string(),
        ];
        assert_eq!(pre_scan_lang(args.into_iter()), None);
    }

    #[test]
    fn command_builds_and_accepts_defaults() {
        let m = command()
            .try_get_matches_from(["sadp-discover"])
            .expect("parse");
        let scan = ScanArgs::from_matches(&m).expect("scan args");
        assert_eq!(scan.timeout, Duration::from_millis(DEFAULT_TIMEOUT_MS));
        assert_eq!(scan.port, 37020);
        assert_eq!(scan.families, vec![Family::V4, Family::V6]);
        assert!(scan.wants_tui());
    }

    #[test]
    fn parses_family_and_flags() {
        let m = command()
            .try_get_matches_from(["sadp-discover", "--family", "v6", "--no-tui", "--json"])
            .expect("parse");
        let scan = ScanArgs::from_matches(&m).expect("scan args");
        assert_eq!(scan.families, vec![Family::V6]);
        assert!(scan.no_tui);
        assert!(scan.json);
        assert!(!scan.wants_tui());
    }

    #[test]
    fn rejects_bad_numbers() {
        let m = command()
            .try_get_matches_from(["sadp-discover", "--timeout", "abc"])
            .expect("clap accepts string");
        assert!(ScanArgs::from_matches(&m).is_err());
    }

    #[test]
    fn rejects_json_and_csv_together() {
        let m = command()
            .try_get_matches_from(["sadp-discover", "--json", "--csv"])
            .expect("clap accepts flags");
        assert!(ScanArgs::from_matches(&m).is_err());
    }

    #[test]
    fn repeat_shorter_than_timeout_clamps_the_window() {
        let m = command()
            .try_get_matches_from(["sadp-discover", "--timeout", "3000", "--repeat", "1000"])
            .expect("parse");
        let scan = ScanArgs::from_matches(&m).expect("scan args");
        assert_eq!(scan.effective_timeout(), Duration::from_millis(1000));
        assert_eq!(
            scan.scan_config().expect("valid").timeout,
            Duration::from_millis(1000)
        );
        assert_eq!(scan.notes().len(), 1, "应当提示窗口被收窄");
    }

    #[test]
    fn repeat_longer_than_timeout_keeps_the_window() {
        let m = command()
            .try_get_matches_from(["sadp-discover", "--timeout", "1000", "--repeat", "3000"])
            .expect("parse");
        let scan = ScanArgs::from_matches(&m).expect("scan args");
        assert_eq!(scan.effective_timeout(), Duration::from_millis(1000));
        assert!(scan.notes().is_empty());
    }

    #[test]
    fn interface_accepts_comma_separated_values() {
        let m = command()
            .try_get_matches_from(["sadp-discover", "-i", "eth0,eth1"])
            .expect("parse");
        let scan = ScanArgs::from_matches(&m).expect("scan args");
        assert_eq!(
            scan.interfaces,
            vec!["eth0".to_string(), "eth1".to_string()]
        );
    }
}
