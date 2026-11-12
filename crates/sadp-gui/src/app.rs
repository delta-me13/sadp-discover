//! 三阶段 G1/G2：egui 界面。
//!
//! 设计约束（见 docs/ARCHITECTURE.md ADR-004）：GUI 只消费 sadp-core 的 ScanEvent 与 view 视图函数，
//! 扫描线程、去重、合并、国际化全部复用，不在 UI 层重写。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui;
use egui_extras::{Column, TableBuilder};
use sadp_core::scan::{ScanConfig, ScanEvent, Scanner, Session};
use sadp_core::view::{SortKey, ViewOptions, visible_indices};
use sadp_core::{Device, Lang, Transport, tr, tr_args};

use crate::export;

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
    worker: Option<Worker>,
    session: Session,
    view: ViewOptions,
    filter: String,
    selected: Option<String>,
    lang: Lang,
    toast: Option<(String, Instant)>,
}

impl App {
    pub fn new(ctx: &egui::Context, cfg: ScanConfig, transport: Arc<dyn Transport>) -> Self {
        crate::fonts::install(ctx);
        let view = ViewOptions::default();
        let mut app = Self {
            cfg,
            transport,
            worker: None,
            session: Session::new(),
            view,
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
        let result = (|| -> anyhow::Result<String> {
            let body = if json {
                export::json(self.session.devices())?
            } else {
                export::csv(self.session.devices())
            };
            let name = format!(
                "sadp-discover-gui-{}.{}",
                stamp(),
                if json { "json" } else { "csv" }
            );
            std::fs::write(&name, body)?;
            Ok(name)
        })();
        match result {
            Ok(path) => {
                tracing::info!(%path, "已导出");
                self.set_toast(tr_args("tui.status.exported", &[("path", &path)]));
            }
            Err(err) => {
                let message = err.to_string();
                self.set_toast(tr_args("tui.export.failed", &[("reason", &message)]));
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
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.draw(ctx);
    }
}

impl App {
    /// 与 eframe 解耦的绘制入口：测试可以在没有窗口的情况下跑完整 UI pass。
    pub fn draw(&mut self, ctx: &egui::Context) {
        self.drain();
        self.view.filter = self.filter.clone();

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
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
                    self.copy(ctx);
                }
                ui.separator();
                if ui
                    .button(format!("{} ({})", tr("gui.action.lang"), self.lang))
                    .clicked()
                {
                    self.toggle_lang();
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

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
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
            });
        });

        let devices: Vec<Device> = self.session.devices().to_vec();
        let visible = visible_indices(&devices, &self.view);

        egui::SidePanel::right("detail")
            .resizable(true)
            .default_width(340.0)
            .show(ctx, |ui| {
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

        egui::CentralPanel::default().show(ctx, |ui| {
            if visible.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(tr("tui.empty"));
                });
                return;
            }
            let mut clicked: Option<String> = None;
            let row_height = 22.0;
            TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
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
                        let selected = self.selected.as_ref() == Some(&key);
                        row.col(|ui| {
                            if ui.selectable_label(selected, device.primary_ip()).clicked() {
                                clicked = Some(key.clone());
                            }
                        });
                        row.col(|ui| {
                            ui.label(
                                device
                                    .mac
                                    .map(|m| m.to_string())
                                    .unwrap_or_else(|| tr("tui.none")),
                            );
                        });
                        row.col(|ui| {
                            ui.label(
                                device
                                    .display_model()
                                    .map(str::to_string)
                                    .unwrap_or_else(|| tr("tui.none")),
                            );
                        });
                        row.col(|ui| {
                            ui.label(export::via_label(device));
                        });
                        row.col(|ui| {
                            ui.label(match device.activated {
                                Some(true) => tr("tui.yes"),
                                Some(false) => tr("tui.no"),
                                None => tr("tui.unknown"),
                            });
                        });
                    });
                });
            if let Some(key) = clicked {
                self.selected = Some(key);
            }
        });

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
                    device.sn.clone().unwrap_or_else(|| tr("tui.none")),
                );
                row(
                    tr("tui.detail.mac"),
                    device
                        .mac
                        .map(|m| m.to_string())
                        .unwrap_or_else(|| tr("tui.none")),
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
                        .unwrap_or_else(|| tr("tui.none")),
                );
                row(
                    tr("tui.detail.dhcp"),
                    match device.dhcp {
                        Some(true) => tr("tui.yes"),
                        Some(false) => tr("tui.no"),
                        None => tr("tui.unknown"),
                    },
                );
                let ports = match (device.sdk_port, device.http_port) {
                    (None, None) => tr("tui.none"),
                    (sdk, http) => format!(
                        "{} / {}",
                        sdk.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                        http.map(|p| p.to_string()).unwrap_or_else(|| "-".into())
                    ),
                };
                row(tr("tui.detail.ports"), ports);
                let channels = match (device.analog_channels, device.digital_channels) {
                    (None, None) => tr("tui.none"),
                    (a, d) => format!(
                        "{} / {}",
                        a.map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
                        d.map(|p| p.to_string()).unwrap_or_else(|| "-".into())
                    ),
                };
                row(tr("tui.detail.channels"), channels);
                row(
                    tr("tui.detail.firmware"),
                    device.firmware.clone().unwrap_or_else(|| tr("tui.none")),
                );
                row(
                    tr("tui.detail.model"),
                    device
                        .display_model()
                        .map(str::to_string)
                        .unwrap_or_else(|| tr("tui.none")),
                );
                row(
                    tr("tui.detail.description"),
                    device.description.clone().unwrap_or_else(|| tr("tui.none")),
                );
                row(tr("tui.detail.via"), export::via_label(device).to_string());
                row(
                    tr("tui.detail.interfaces"),
                    if device.seen_on.is_empty() {
                        tr("tui.none")
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
        let mut app = App::new(&ctx, cfg, transport);

        let _ = ctx.run(raw_input(), |ctx| app.draw(ctx));
        std::thread::sleep(Duration::from_millis(400));
        let _ = ctx.run(raw_input(), |ctx| app.draw(ctx));

        assert!(app.session.round >= 1, "扫描线程应至少完成一轮");
        assert!(
            !app.session.devices().is_empty(),
            "MockTransport 的假设备应进入 DeviceStore"
        );

        // 选中第一台后详情面板也要能渲染。
        app.selected = app.session.devices().first().map(|d| d.key().0.clone());
        let _ = ctx.run(raw_input(), |ctx| app.draw(ctx));
    }

    #[test]
    fn filter_reduces_visible_rows() {
        let ctx = egui::Context::default();
        let transport: Arc<dyn Transport> = Arc::new(sadp_net::MockTransport::single());
        let mut app = App::new(&ctx, ScanConfig::default(), transport);
        std::thread::sleep(Duration::from_millis(400));

        app.filter = "没有这台设备".to_string();
        app.view.filter = app.filter.clone();
        let visible = visible_indices(app.session.devices(), &app.view);
        assert!(visible.is_empty(), "过滤词不匹配时不应有可见行");
    }
}
