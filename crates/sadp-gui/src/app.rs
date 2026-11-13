//! 三阶段 G1/G2：egui 界面。
//!
//! 设计约束（见 docs/ARCHITECTURE.md ADR-004）：GUI 只消费 sadp-core 的 ScanEvent 与 view 视图函数，
//! 扫描线程、去重、合并、国际化全部复用，不在 UI 层重写。

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui;
use egui_extras::{Column, TableBuilder};
use sadp_core::scan::{ScanConfig, ScanEvent, Scanner, Session};
use sadp_core::view::{SortKey, ViewOptions, visible_indices};
use sadp_core::{Device, Family, InterfaceInfo, Lang, Transport, tr, tr_args};
use sadp_net::{InterfaceSelector, enumerate_interfaces};

use crate::{browser, export};

/// 设置变更后重建传输层；`None` 表示沿用现有 transport（测试用 MockTransport）。
pub type TransportFactory =
    Arc<dyn Fn(&ScanConfig) -> anyhow::Result<Arc<dyn Transport>> + Send + Sync>;

/// 深/浅两套配色。默认深色，切换只影响当前会话。
#[derive(Clone, Copy, PartialEq, Eq)]
enum Theme {
    Dark,
    Light,
}

impl Theme {
    const fn toggle(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    fn apply(self, ctx: &egui::Context) {
        // egui 0.37 由「主题偏好」统一管理配色；直接 set_visuals 会在下一帧被覆盖。
        let preference = match self {
            Self::Dark => egui::ThemePreference::Dark,
            Self::Light => egui::ThemePreference::Light,
        };
        ctx.set_theme(preference);
    }
}

/// 扫描线程发回主线程的消息。
enum Msg {
    Events(Vec<ScanEvent>),
    Failed(String),
    Finished,
}

/// 扫描线程句柄。poll 每次最多阻塞 200 ms，停止就是置标志 + join。
struct Worker {
    rx: Receiver<Msg>,
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    fn spawn(cfg: ScanConfig, transport: Arc<dyn Transport>) -> Self {
        let (tx, rx): (Sender<Msg>, Receiver<Msg>) = unbounded();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let join = std::thread::spawn(move || {
            let mut scanner = match Scanner::start(cfg, transport) {
                Ok(scanner) => scanner,
                Err(err) => {
                    let _ = tx.send(Msg::Failed(err.to_string()));
                    let _ = tx.send(Msg::Finished);
                    return;
                }
            };
            while !flag.load(Ordering::Relaxed) && !scanner.is_finished() {
                let events = scanner.poll(Duration::from_millis(200));
                if !events.is_empty() && tx.send(Msg::Events(events)).is_err() {
                    break;
                }
            }
            scanner.stop();
            let _ = tx.send(Msg::Finished);
        });
        Self {
            rx,
            stop,
            join: Some(join),
        }
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct App {
    cfg: ScanConfig,
    transport: Arc<dyn Transport>,
    factory: Option<TransportFactory>,
    worker: Option<Worker>,
    session: Session,
    view: ViewOptions,
    filter: String,
    selected: Option<String>,
    lang: Lang,
    toast: Option<(String, Instant)>,
    show_settings: bool,
    ifaces: Vec<InterfaceInfo>,
    iface_sel: BTreeSet<String>,
    fam_v4: bool,
    fam_v6: bool,
    timeout_ms: u64,
    repeat_ms: u64,
    port: u16,
    theme: Theme,
}

impl App {
    pub fn new(
        ctx: &egui::Context,
        cfg: ScanConfig,
        transport: Arc<dyn Transport>,
        factory: Option<TransportFactory>,
    ) -> Self {
        crate::fonts::install(ctx);
        let theme = Theme::Dark;
        theme.apply(ctx);

        // 设置面板的初始值来自实际生效的配置；Auto 会展开成当前可用网卡。
        let ifaces = enumerate_interfaces(&InterfaceSelector::All).unwrap_or_default();
        let iface_sel = enumerate_interfaces(&cfg.interfaces)
            .unwrap_or_default()
            .into_iter()
            .map(|iface| iface.name)
            .collect();

        let mut app = Self {
            fam_v4: cfg.families.contains(&Family::V4),
            fam_v6: cfg.families.contains(&Family::V6),
            timeout_ms: u64::try_from(cfg.timeout.as_millis()).unwrap_or(3000),
            repeat_ms: u64::try_from(cfg.repeat.unwrap_or(Duration::from_secs(1)).as_millis())
                .unwrap_or(1000),
            port: cfg.port,
            ifaces,
            iface_sel,
            show_settings: false,
            factory,
            theme,
            cfg,
            transport,
            worker: None,
            session: Session::new(),
            view: ViewOptions::default(),
            filter: String::new(),
            selected: None,
            lang: sadp_core::i18n::current(),
            toast: None,
        };
        app.start_scan();
        app
    }

    fn is_scanning(&self) -> bool {
        self.worker.is_some()
    }

    fn start_scan(&mut self) {
        if self.worker.is_some() {
            return;
        }
        self.session = Session::new();
        self.selected = None;
        self.worker = Some(Worker::spawn(self.cfg.clone(), Arc::clone(&self.transport)));
    }

    fn stop_scan(&mut self) {
        if let Some(mut worker) = self.worker.take() {
            worker.stop();
        }
    }

    fn drain(&mut self) {
        let Some(worker) = self.worker.as_ref() else {
            return;
        };
        let messages: Vec<Msg> = worker.rx.try_iter().collect();
        let mut finished = false;
        for msg in messages {
            match msg {
                Msg::Events(events) => {
                    for event in &events {
                        self.session.apply(event);
                    }
                }
                Msg::Failed(message) => {
                    tracing::error!(%message, "扫描线程启动失败");
                    self.set_toast(tr_args("tui.status.error", &[("message", &message)]));
                }
                Msg::Finished => finished = true,
            }
        }
        if finished {
            self.worker = None;
        }
    }

    fn set_toast(&mut self, message: String) {
        self.toast = Some((message, Instant::now()));
    }

    fn export(&mut self, json: bool) {
        let ext = if json { "json" } else { "csv" };
        let meta = export::Meta {
            port: self.cfg.port,
            families: &self.cfg.families,
            elapsed: self.session.elapsed(),
            rounds: self.session.round,
        };
        let body = if json {
            export::json(self.session.devices(), &meta).map_err(|e| e.to_string())
        } else {
            export::csv(self.session.devices()).map_err(|e| e.to_string())
        };
        let body = match body {
            Ok(body) => body,
            Err(reason) => {
                self.set_toast(tr_args("tui.export.failed", &[("reason", &reason)]));
                return;
            }
        };

        let default_name = format!("sadp-discover-gui-{}.{ext}", stamp());
        let Some(path) = rfd::FileDialog::new()
            .set_title(tr("gui.export.save_as"))
            .set_file_name(default_name)
            .add_filter(ext.to_uppercase(), &[ext])
            .save_file()
        else {
            return; // 用户取消
        };
        let path_label = path.display().to_string();
        match std::fs::write(&path, body) {
            Ok(()) => {
                tracing::info!(path = %path_label, "已导出");
                self.set_toast(tr_args("tui.status.exported", &[("path", &path_label)]));
            }
            Err(err) => {
                let reason = err.to_string();
                self.set_toast(tr_args("tui.export.failed", &[("reason", &reason)]));
            }
        }
    }

    fn copy(&mut self, ctx: &egui::Context) {
        if self.session.devices().is_empty() {
            self.set_toast(tr("tui.status.nothing_to_copy"));
            return;
        }
        ctx.copy_text(export::clipboard(self.session.devices()));
        self.set_toast(tr("tui.status.copied"));
    }

    fn toggle_lang(&mut self) {
        self.lang = sadp_core::i18n::toggle();
    }

    /// 打开日志目录，方便排障时一键跳转。
    fn open_logs(&mut self) {
        let Some(dir) = crate::logging::log_dir() else {
            self.set_toast(tr("log.unavailable"));
            return;
        };
        if let Err(err) = std::fs::create_dir_all(&dir) {
            let reason = err.to_string();
            self.set_toast(tr_args("log.failed", &[("reason", &reason)]));
            return;
        }
        let path = dir.display().to_string();
        match sadp_core::system::open_dir(&dir) {
            Ok(()) => self.set_toast(tr_args("log.opened", &[("path", &path)])),
            Err(err) => {
                let reason = err.to_string();
                self.set_toast(tr_args("log.failed", &[("reason", &reason)]));
            }
        }
    }

    /// 扫描设置：网卡多选、地址族、窗口/周期、端口。应用后重建传输层并重扫。
    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;

        // 本地编辑，点「应用」才写回；取消不会改配置。
        let ifaces = self.ifaces.clone();
        let mut selected = self.iface_sel.clone();
        let mut fam_v4 = self.fam_v4;
        let mut fam_v6 = self.fam_v6;
        let mut timeout_ms = self.timeout_ms;
        let mut repeat_ms = self.repeat_ms;
        let mut port = self.port;

        egui::Window::new(tr("gui.settings.title"))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.label(tr("gui.settings.interfaces"));
                ui.horizontal(|ui| {
                    if ui.button(tr("gui.settings.select_all")).clicked() {
                        selected = ifaces.iter().map(|i| i.name.clone()).collect();
                    }
                    if ui.button(tr("gui.settings.select_none")).clicked() {
                        selected.clear();
                    }
                });
                egui::ScrollArea::vertical()
                    .max_height(150.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for iface in &ifaces {
                            let mut checked = selected.contains(&iface.name);
                            let label = format!("{}  {}", iface.name, iface_summary(iface));
                            if ui.checkbox(&mut checked, label).changed() {
                                if checked {
                                    selected.insert(iface.name.clone());
                                } else {
                                    selected.remove(&iface.name);
                                }
                            }
                        }
                    });

                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(tr("gui.settings.families"));
                    ui.checkbox(&mut fam_v4, "IPv4");
                    ui.checkbox(&mut fam_v6, "IPv6");
                });
                ui.add(
                    egui::Slider::new(&mut timeout_ms, 200..=60_000)
                        .text(tr("gui.settings.timeout"))
                        .suffix(" ms"),
                );
                ui.add(
                    egui::Slider::new(&mut repeat_ms, 200..=600_000)
                        .text(tr("gui.settings.repeat"))
                        .suffix(" ms"),
                );
                ui.horizontal(|ui| {
                    ui.label(tr("gui.settings.port"));
                    ui.add(egui::DragValue::new(&mut port).range(1..=65_535).speed(1));
                });
                ui.label(egui::RichText::new(tr("gui.settings.hint")).small());
                ui.separator();
                ui.horizontal(|ui| {
                    let valid = !selected.is_empty() && (fam_v4 || fam_v6);
                    if ui
                        .add_enabled(valid, egui::Button::new(tr("gui.settings.apply")))
                        .clicked()
                    {
                        apply = true;
                    }
                    if ui.button(tr("gui.settings.cancel")).clicked() {
                        cancel = true;
                    }
                    if !fam_v4 && !fam_v6 {
                        ui.colored_label(
                            egui::Color32::from_rgb(220, 120, 80),
                            tr("gui.settings.error.family"),
                        );
                    } else if selected.is_empty() {
                        ui.colored_label(
                            egui::Color32::from_rgb(220, 120, 80),
                            tr("gui.settings.error.interface"),
                        );
                    }
                });
            });

        self.show_settings = open && !cancel;
        if apply {
            match self.apply_settings(&selected, fam_v4, fam_v6, timeout_ms, repeat_ms, port) {
                Ok(()) => {
                    self.show_settings = false;
                    self.set_toast(tr("gui.settings.applied"));
                }
                Err(message) => self.set_toast(message),
            }
        }
    }

    fn apply_settings(
        &mut self,
        selected: &BTreeSet<String>,
        fam_v4: bool,
        fam_v6: bool,
        timeout_ms: u64,
        repeat_ms: u64,
        port: u16,
    ) -> Result<(), String> {
        if !fam_v4 && !fam_v6 {
            return Err(tr("gui.settings.error.family"));
        }
        if selected.is_empty() {
            return Err(tr("gui.settings.error.interface"));
        }

        // 与 CLI 的约束一致：窗口不能长于周期。
        let repeat_ms = repeat_ms.clamp(200, 600_000);
        let timeout_ms = timeout_ms.clamp(100, repeat_ms);
        let mut cfg = self.cfg.clone();
        cfg.port = port;
        cfg.timeout = Duration::from_millis(timeout_ms);
        cfg.repeat = Some(Duration::from_millis(repeat_ms));
        cfg.families = match (fam_v4, fam_v6) {
            (true, true) => vec![Family::V4, Family::V6],
            (true, false) => vec![Family::V4],
            _ => vec![Family::V6],
        };
        cfg.interfaces = selector_for(selected, &self.ifaces);
        if let Err(err) = cfg.validate() {
            let reason = err.to_string();
            return Err(tr_args(
                "gui.settings.error.invalid",
                &[("reason", &reason)],
            ));
        }

        self.stop_scan();
        let rebound = match &self.factory {
            Some(factory) => match factory(&cfg) {
                Ok(transport) => Some(transport),
                Err(err) => {
                    // 绑定失败时按旧配置恢复，避免界面停在「已暂停」。
                    let reason = err.to_string();
                    self.worker =
                        Some(Worker::spawn(self.cfg.clone(), Arc::clone(&self.transport)));
                    return Err(tr_args("gui.settings.error.rebind", &[("reason", &reason)]));
                }
            },
            None => None,
        };
        if let Some(transport) = rebound {
            self.transport = transport;
        }

        self.cfg = cfg;
        self.timeout_ms = timeout_ms;
        self.repeat_ms = repeat_ms;
        self.port = port;
        self.fam_v4 = fam_v4;
        self.fam_v6 = fam_v6;
        self.iface_sel = selected.clone();
        self.session = Session::new();
        self.selected = None;
        self.start_scan();
        Ok(())
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(ui);
    }
}

/// 选中的网卡正好等于「自动」范围时用 `Auto`，否则显式 `Only`。
fn selector_for(selected: &BTreeSet<String>, ifaces: &[InterfaceInfo]) -> InterfaceSelector {
    let non_loopback: BTreeSet<String> = ifaces
        .iter()
        .filter(|iface| !iface.loopback)
        .map(|iface| iface.name.clone())
        .collect();
    let auto = if non_loopback.is_empty() {
        ifaces.iter().map(|iface| iface.name.clone()).collect()
    } else {
        non_loopback
    };
    if *selected == auto {
        InterfaceSelector::Auto
    } else {
        InterfaceSelector::Only(selected.iter().cloned().collect())
    }
}

fn iface_summary(iface: &InterfaceInfo) -> String {
    let mut parts = Vec::new();
    if let Some(ip) = iface.ipv4 {
        parts.push(ip.to_string());
    }
    for ip in iface.ipv6.iter().take(2) {
        parts.push(ip.to_string());
    }
    if iface.loopback {
        parts.push(tr("interfaces.loopback"));
    }
    if parts.is_empty() {
        tr("common.none")
    } else {
        parts.join(", ")
    }
}

impl App {
    /// 与 eframe 解耦的绘制入口：测试用 Context::run_ui 在没有窗口时跑完整 UI pass。
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.drain();
        self.view.filter = self.filter.clone();

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                let scanning = self.is_scanning();
                let label = if scanning {
                    tr("gui.action.pause")
                } else {
                    tr("gui.action.scan")
                };
                if ui.button(label).clicked() {
                    if scanning {
                        self.stop_scan();
                    } else {
                        self.start_scan();
                    }
                }

                ui.separator();
                if ui.button(tr("gui.action.export_json")).clicked() {
                    self.export(true);
                }
                if ui.button(tr("gui.action.export_csv")).clicked() {
                    self.export(false);
                }
                if ui.button(tr("gui.action.copy")).clicked() {
                    self.copy(&ctx);
                }
                ui.separator();
                if ui
                    .button(format!("{} ({})", tr("gui.action.lang"), self.lang))
                    .clicked()
                {
                    self.toggle_lang();
                }

                ui.separator();
                if ui.button(tr("gui.action.settings")).clicked() {
                    // 打开时刷新网卡列表，热插拔/新 VPN 不用重启程序。
                    self.ifaces = enumerate_interfaces(&InterfaceSelector::All).unwrap_or_default();
                    self.show_settings = true;
                }
                let theme_label = if self.theme == Theme::Dark {
                    tr("gui.action.theme_light")
                } else {
                    tr("gui.action.theme_dark")
                };
                if ui.button(theme_label).clicked() {
                    self.theme = self.theme.toggle();
                    self.theme.apply(&ctx);
                }

                ui.separator();
                ui.label(tr("tui.filter.prompt"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .desired_width(200.0)
                        .hint_text(tr("tui.filter.prompt")),
                );

                ui.separator();
                egui::ComboBox::from_id_salt("sort")
                    .selected_text(tr(self.view.sort.label_key()))
                    .show_ui(ui, |ui| {
                        for key in SortKey::ALL {
                            ui.selectable_value(&mut self.view.sort, key, tr(key.label_key()));
                        }
                    });
                if ui.button(self.view.dir.arrow()).clicked() {
                    self.view.dir = self.view.dir.flip();
                }
            });
            ui.add_space(4.0);
        });

        let mut log_clicked = false;
        egui::Panel::bottom("status").show(ui, |ui| {
            let total = self.session.devices().len();
            ui.horizontal(|ui| {
                let scanning = self.is_scanning();
                let text = if scanning {
                    tr_args(
                        "tui.status.scanning",
                        &[
                            ("count", &total.to_string()),
                            ("round", &self.session.round.to_string()),
                        ],
                    )
                } else {
                    tr_args("tui.status.paused", &[("count", &total.to_string())])
                };
                ui.label(text);
                ui.separator();
                ui.label(tr_args(
                    "tui.header.devices",
                    &[("count", &total.to_string())],
                ));
                if total == 0 && self.session.round > 0 && self.session.sadp_packets == 0 {
                    ui.separator();
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 160, 60),
                        tr("tui.status.no_responses"),
                    );
                }
                if let Some((message, at)) = self.toast.as_ref()
                    && at.elapsed() < Duration::from_secs(8)
                {
                    ui.separator();
                    ui.label(message);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(tr("log.action"))
                        .on_hover_text(tr("log.action_tip"))
                        .clicked()
                    {
                        log_clicked = true;
                    }
                });
            });
        });
        if log_clicked {
            self.open_logs();
        }

        let devices: Vec<Device> = self.session.devices().to_vec();
        let visible = visible_indices(&devices, &self.view);

        egui::Panel::right("detail")
            .resizable(true)
            .default_size(340.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.heading(tr("tui.detail.title"));
                ui.separator();
                match self
                    .selected
                    .as_ref()
                    .and_then(|key| devices.iter().find(|d| d.key().0 == *key))
                {
                    Some(device) => detail(ui, device),
                    None => {
                        ui.label(tr("tui.detail.empty"));
                    }
                }
            });

        egui::CentralPanel::default().show(ui, |ui| {
            if visible.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(tr("tui.empty"));
                });
                return;
            }
            // 表格文字不参与文本选择：否则点击 IP/型号等文字会被 Label 拦下，
            // 整行点击与双击 IP 都只有单元格空白处才生效。
            ui.style_mut().interaction.selectable_labels = false;

            let mut clicked: Option<String> = None;
            let mut open_web: Option<String> = None;
            let row_height = 22.0;
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .sense(egui::Sense::click())
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(Column::initial(180.0).at_least(120.0))
                .column(Column::initial(150.0).at_least(120.0))
                .column(Column::initial(200.0).at_least(120.0))
                .column(Column::initial(70.0).at_least(60.0))
                .column(Column::initial(70.0).at_least(60.0))
                .header(24.0, |mut header| {
                    for key in [
                        "tui.col.ip",
                        "tui.col.mac",
                        "tui.col.model",
                        "tui.col.via",
                        "tui.col.activated",
                    ] {
                        header.col(|ui| {
                            ui.strong(tr(key));
                        });
                    }
                })
                .body(|body| {
                    body.rows(row_height, visible.len(), |mut row| {
                        let index = row.index();
                        let Some(device) = visible.get(index).and_then(|i| devices.get(*i)) else {
                            return;
                        };
                        let key = device.key().0.clone();
                        row.set_selected(self.selected.as_ref() == Some(&key));
                        let (_, ip_response) = row.col(|ui| {
                            ui.label(device.primary_ip())
                                .on_hover_text(tr("gui.tip.open_web"));
                        });
                        row.col(|ui| {
                            ui.label(
                                device
                                    .mac
                                    .map(|m| m.to_string())
                                    .unwrap_or_else(|| tr("common.none")),
                            );
                        });
                        row.col(|ui| {
                            ui.label(
                                device
                                    .display_model()
                                    .map(str::to_string)
                                    .unwrap_or_else(|| tr("common.none")),
                            );
                        });
                        row.col(|ui| {
                            ui.label(sadp_core::export::via_label(device));
                        });
                        row.col(|ui| {
                            ui.label(match device.activated {
                                Some(true) => tr("common.yes"),
                                Some(false) => tr("common.no"),
                                None => tr("common.unknown"),
                            });
                        });

                        // 双击 IP：用系统默认浏览器打开设备 Web 后台。
                        if ip_response.double_clicked()
                            && let Some(url) = browser::web_url(device)
                        {
                            open_web = Some(url);
                        }
                        // 整行任意位置单击都选中（详情面板随之更新）。
                        if row.response().clicked() {
                            clicked = Some(key);
                        }
                    });
                });
            if let Some(key) = clicked {
                self.selected = Some(key);
            }
            if let Some(url) = open_web {
                match sadp_core::system::open_url(&url) {
                    Ok(()) => self.set_toast(tr_args("gui.status.opened", &[("url", &url)])),
                    Err(err) => {
                        let reason = err.to_string();
                        self.set_toast(tr_args("gui.error.open_web", &[("reason", &reason)]));
                    }
                }
            }
        });

        self.settings_window(&ctx);

        ctx.request_repaint_after(Duration::from_millis(if self.is_scanning() {
            100
        } else {
            500
        }));
    }
}

fn detail(ui: &mut egui::Ui, device: &Device) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("detail-grid")
            .num_columns(2)
            .spacing([10.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                let mut row = |label: String, value: String| {
                    ui.label(label);
                    ui.label(value);
                    ui.end_row();
                };
                row(
                    tr("tui.detail.sn"),
                    device.sn.clone().unwrap_or_else(|| tr("common.none")),
                );
                row(
                    tr("tui.detail.mac"),
                    device
                        .mac
                        .map(|m| m.to_string())
                        .unwrap_or_else(|| tr("common.none")),
                );
                row(tr("tui.col.ip"), device.primary_ip());
                if !device.ipv6.is_empty() {
                    let list = device
                        .ipv6
                        .iter()
                        .map(|e| e.addr.to_string())
                        .collect::<Vec<_>>()
                        .join("\n");
                    row("IPv6".to_string(), list);
                }
                row(
                    tr("tui.detail.gateway"),
                    device
                        .gateway
                        .map(|ip| ip.to_string())
                        .unwrap_or_else(|| tr("common.none")),
                );
                row(
                    tr("tui.detail.dhcp"),
                    match device.dhcp {
                        Some(true) => tr("common.yes"),
                        Some(false) => tr("common.no"),
                        None => tr("common.unknown"),
                    },
                );
                let ports = match (device.sdk_port, device.http_port) {
                    (None, None) => tr("common.none"),
                    (sdk, http) => format!(
                        "{} / {}",
                        sdk.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                        http.map(|p| p.to_string()).unwrap_or_else(|| "-".into())
                    ),
                };
                row(tr("tui.detail.ports"), ports);
                let channels = match (device.analog_channels, device.digital_channels) {
                    (None, None) => tr("common.none"),
                    (a, d) => format!(
                        "{} / {}",
                        a.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                        d.map(|p| p.to_string()).unwrap_or_else(|| "-".into())
                    ),
                };
                row(tr("tui.detail.channels"), channels);
                row(
                    tr("tui.detail.firmware"),
                    device.firmware.clone().unwrap_or_else(|| tr("common.none")),
                );
                row(
                    tr("tui.detail.model"),
                    device
                        .display_model()
                        .map(str::to_string)
                        .unwrap_or_else(|| tr("common.none")),
                );
                row(
                    tr("tui.detail.description"),
                    device
                        .description
                        .clone()
                        .unwrap_or_else(|| tr("common.none")),
                );
                row(
                    tr("tui.detail.via"),
                    sadp_core::export::via_label(device).to_string(),
                );
                row(
                    tr("tui.detail.interfaces"),
                    if device.seen_on.is_empty() {
                        tr("common.none")
                    } else {
                        device.seen_on.join(", ")
                    },
                );
                row(
                    tr("tui.detail.first_seen"),
                    elapsed_label(device.first_seen),
                );
                row(tr("tui.detail.last_seen"), elapsed_label(device.last_seen));
            });
    });
}

fn elapsed_label(at: Instant) -> String {
    let secs = at.elapsed().as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}h{}m", secs / 3600, secs % 3600 / 60)
    }
}

fn stamp() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_input() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1180.0, 740.0),
            )),
            ..Default::default()
        }
    }

    /// 无窗口渲染：MockTransport 喂一台假设备，跑完整 UI pass，确认不 panic 且事件进了 Session。
    #[test]
    fn renders_headless_and_receives_devices() {
        const IPC_V4: &str = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/probe_match_ipc_v4.xml"
        ));

        let ctx = egui::Context::default();
        let mock = sadp_net::MockTransport::single();
        mock.push_bytes(
            sadp_core::Family::V4,
            "192.168.1.64:37020".parse().unwrap(),
            IPC_V4.as_bytes(),
        );
        let transport: Arc<dyn Transport> = Arc::new(mock);
        let cfg = ScanConfig {
            timeout: Duration::from_millis(120),
            repeat: Some(Duration::from_millis(120)),
            ..ScanConfig::default()
        };
        let mut app = App::new(&ctx, cfg, transport, None);

        // 0.37 起 FullOutput 里的纹理增量必须被消费，否则 Drop 会 panic（无渲染器时清空即可）。
        let mut out = ctx.run_ui(raw_input(), |ui| app.draw(ui));
        out.textures_delta.clear();
        std::thread::sleep(Duration::from_millis(400));
        let mut out = ctx.run_ui(raw_input(), |ui| app.draw(ui));
        out.textures_delta.clear();

        assert!(app.session.round >= 1, "扫描线程应至少完成一轮");
        assert!(
            !app.session.devices().is_empty(),
            "MockTransport 的假设备应进入 DeviceStore"
        );

        // 选中第一台后详情面板也要能渲染。
        app.selected = app.session.devices().first().map(|d| d.key().0.clone());
        let mut out = ctx.run_ui(raw_input(), |ui| app.draw(ui));
        out.textures_delta.clear();
    }

    fn test_iface(name: &str, index: u32, ip: &str, loopback: bool) -> InterfaceInfo {
        InterfaceInfo {
            name: name.to_string(),
            index,
            ipv4: Some(ip.parse().unwrap()),
            netmask: Some("255.255.255.0".parse().unwrap()),
            broadcast: None,
            ipv6: Vec::new(),
            loopback,
        }
    }

    #[test]
    fn selector_for_prefers_auto_when_every_non_loopback_is_selected() {
        let ifaces = vec![
            test_iface("eth0", 5, "192.168.1.10", false),
            test_iface("wlan0", 6, "10.0.0.2", false),
            test_iface("lo", 1, "127.0.0.1", true),
        ];
        let all: BTreeSet<String> = ifaces
            .iter()
            .filter(|iface| !iface.loopback)
            .map(|iface| iface.name.clone())
            .collect();
        assert_eq!(selector_for(&all, &ifaces), InterfaceSelector::Auto);

        let only_eth: BTreeSet<String> = ["eth0".to_string()].into_iter().collect();
        assert_eq!(
            selector_for(&only_eth, &ifaces),
            InterfaceSelector::Only(vec!["eth0".to_string()])
        );
    }

    #[test]
    fn apply_settings_updates_cfg_and_restarts_scanning() {
        let ctx = egui::Context::default();
        let transport: Arc<dyn Transport> = Arc::new(sadp_net::MockTransport::single());
        let mut app = App::new(&ctx, ScanConfig::default(), transport, None);
        if app.iface_sel.is_empty() {
            // CI 的极简环境可能没有可用网卡；注入一块假网卡，保证用例本身可执行。
            app.ifaces = vec![test_iface("eth0", 5, "192.168.1.10", false)];
            app.iface_sel = ["eth0".to_string()].into_iter().collect();
        }
        let selected = app.iface_sel.clone();

        app.apply_settings(&selected, true, false, 500, 1000, 37_021)
            .expect("valid settings");
        assert_eq!(app.cfg.port, 37_021);
        assert_eq!(app.cfg.families, vec![Family::V4]);
        assert_eq!(app.cfg.timeout, Duration::from_millis(500));
        assert_eq!(app.cfg.repeat, Some(Duration::from_millis(1000)));
        assert!(app.is_scanning(), "应用设置后应自动重新扫描");

        // 窗口长于周期时自动收窄到周期。
        app.apply_settings(&selected, true, true, 5000, 1000, 37_020)
            .expect("valid settings");
        assert_eq!(app.cfg.timeout, Duration::from_millis(1000));
        assert_eq!(app.cfg.families, vec![Family::V4, Family::V6]);

        app.stop_scan();
    }

    #[test]
    fn apply_settings_rejects_empty_family_or_interface() {
        let ctx = egui::Context::default();
        let transport: Arc<dyn Transport> = Arc::new(sadp_net::MockTransport::single());
        let mut app = App::new(&ctx, ScanConfig::default(), transport, None);
        if app.iface_sel.is_empty() {
            app.ifaces = vec![test_iface("eth0", 5, "192.168.1.10", false)];
            app.iface_sel = ["eth0".to_string()].into_iter().collect();
        }
        let selected = app.iface_sel.clone();
        assert!(
            app.apply_settings(&selected, false, false, 500, 1000, 37020)
                .is_err()
        );
        assert!(
            app.apply_settings(&BTreeSet::new(), true, true, 500, 1000, 37020)
                .is_err()
        );
        // 失败不应停掉扫描
        assert!(app.is_scanning());
        app.stop_scan();
    }

    fn rendered_text(out: &egui::FullOutput) -> Vec<String> {
        fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        walk(shape, out);
                    }
                }
                _ => {}
            }
        }
        let mut texts = Vec::new();
        for clipped in &out.shapes {
            walk(&clipped.shape, &mut texts);
        }
        texts
    }

    #[test]
    fn panels_render_detail_and_status_bar() {
        let ctx = egui::Context::default();
        let transport: Arc<dyn Transport> = Arc::new(sadp_net::MockTransport::single());
        let mut app = App::new(&ctx, ScanConfig::default(), transport, None);
        std::thread::sleep(Duration::from_millis(400));
        app.selected = app.session.devices().first().map(|d| d.key().0.clone());

        let mut out = ctx.run_ui(raw_input(), |ui| app.draw(ui));
        out.textures_delta.clear();
        let texts = rendered_text(&out);
        assert!(
            texts
                .iter()
                .any(|t| t.contains("Scanning") || t.contains("扫描中")),
            "底部状态栏应渲染: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "Details" || t == "详情"),
            "右侧详情面板应渲染: {texts:?}"
        );
        app.stop_scan();
    }

    #[test]
    fn settings_window_renders_headlessly() {
        let ctx = egui::Context::default();
        let transport: Arc<dyn Transport> = Arc::new(sadp_net::MockTransport::single());
        let mut app = App::new(&ctx, ScanConfig::default(), transport, None);
        app.show_settings = true;

        // 窗口/Area 首帧只登记，第二帧才产生绘制形状。
        for _ in 0..2 {
            let mut out = ctx.run_ui(raw_input(), |ui| app.draw(ui));
            out.textures_delta.clear();
        }
        let mut out = ctx.run_ui(raw_input(), |ui| app.draw(ui));
        out.textures_delta.clear();
        let texts = rendered_text(&out);
        assert!(
            texts
                .iter()
                .any(|t| t == "Scan settings" || t == "扫描设置"),
            "设置窗口应渲染: {texts:?}"
        );
        app.stop_scan();
    }

    #[test]
    fn filter_reduces_visible_rows() {
        let ctx = egui::Context::default();
        let transport: Arc<dyn Transport> = Arc::new(sadp_net::MockTransport::single());
        let mut app = App::new(&ctx, ScanConfig::default(), transport, None);
        std::thread::sleep(Duration::from_millis(400));

        app.filter = "没有这台设备".to_string();
        app.view.filter = app.filter.clone();
        let visible = visible_indices(app.session.devices(), &app.view);
        assert!(visible.is_empty(), "过滤词不匹配时不应有可见行");
    }

    /// GUI 源码里用到的 key 必须在两种语言下都存在。
    ///
    /// 之前表格把「是/否」写成了不存在的 `tui.yes` / `tui.no`，界面上直接露出 key 本身；
    /// 这个测试扫描 `tr("…")` / `tr_args("…")` 的字面量，防止同类漏配再次进入。
    #[test]
    fn translation_keys_used_in_sources_exist() {
        static LOCALE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCALE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut keys = std::collections::BTreeSet::new();
        for entry in std::fs::read_dir(&src).expect("read src dir") {
            let path = entry.expect("dir entry").path();
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("read source file");
            // 跳过测试模块，避免把测试自身构造的字符串当成 key。
            let production = text.split("#[cfg(test)]").next().unwrap_or(&text);
            keys.extend(tr_keys(production));
        }
        assert!(keys.len() > 20, "解析到的 key 数量异常: {}", keys.len());

        for lang in sadp_core::Lang::ALL {
            sadp_core::i18n::set(lang);
            for key in &keys {
                assert!(
                    sadp_core::i18n::has_key(key),
                    "{} 缺少 GUI 使用的翻译 key `{key}`",
                    lang.code()
                );
            }
        }
        sadp_core::i18n::set(sadp_core::Lang::EnUs);
    }

    /// 提取 `tr("…")` / `tr_args("…")` 的字面量 key；动态 key（变量、方法调用）忽略。
    fn tr_keys(text: &str) -> Vec<String> {
        let needles = ["tr_args(", concat!("tr", "(")];
        let mut keys = Vec::new();
        for needle in needles {
            for (idx, _) in text.match_indices(needle) {
                if text[..idx]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_')
                {
                    continue;
                }
                let rest = text[idx + needle.len()..].trim_start();
                let Some(rest) = rest.strip_prefix('"') else {
                    continue;
                };
                let Some(end) = rest.find('"') else {
                    continue;
                };
                let key = &rest[..end];
                if !key.is_empty()
                    && key
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
                {
                    keys.push(key.to_string());
                }
            }
        }
        keys.sort();
        keys.dedup();
        keys
    }
}
