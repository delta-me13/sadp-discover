//! 交互 TUI（ratatui + crossterm）。
//!
//! 设计约束（架构文档 §6.5）：
//! - 100 ms 轮询同时充当渲染节拍，包风暴不会导致逐包重绘；
//! - 所有文案走 i18n，`L` 键即时切换；
//! - 只读同一份 `ScanEvent` 流，三阶段 GUI 复用同一套引擎与视图逻辑。
//!
//! 渲染被抽成纯函数 `render`，测试可用 `TestBackend` 在没有线程的情况下快照。

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Paragraph, Row, Table, TableState};
use ratatui::{Frame, Terminal};
use tui_input::Input;
use tui_input::backend::crossterm::to_input_request;

use sadp_core::i18n::{self, tr, tr_args};
use sadp_core::view::{ViewOptions, visible_indices};
use sadp_core::{Device, ScanConfig, ScanEvent, Scanner, Session, Transport};

use crate::clipboard;
use crate::output::{self, Meta};

const STATUS_TTL: Duration = Duration::from_secs(4);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Normal,
    Filter,
}

pub struct App {
    cfg: ScanConfig,
    transport: Arc<dyn Transport>,
    scanner: Option<Scanner>,
    session: Session,
    view: ViewOptions,
    table_state: TableState,
    mode: Mode,
    filter_input: Input,
    status: Option<(String, Instant)>,
    paused: bool,
    should_quit: bool,
}

impl App {
    pub fn new(
        cfg: ScanConfig,
        transport: Arc<dyn Transport>,
    ) -> Result<Self, sadp_core::CoreError> {
        let scanner = Scanner::start(cfg.clone(), Arc::clone(&transport))?;
        Ok(Self {
            cfg,
            transport,
            scanner: Some(scanner),
            session: Session::new(),
            view: ViewOptions::default(),
            table_state: TableState::default().with_selected(Some(0)),
            mode: Mode::Normal,
            filter_input: Input::default(),
            status: None,
            paused: false,
            should_quit: false,
        })
    }

    /// 运行到用户退出。返回是否发现了至少一台设备（决定退出码）。
    pub fn run(mut self) -> io::Result<bool> {
        let mut stdout = io::stdout();
        terminal::enable_raw_mode()?;
        execute!(stdout, EnterAlternateScreen, cursor::Hide)?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // panic 时恢复终端，否则用户会得到一片乱码的 shell。
        let prev_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = terminal::disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show);
            prev_hook(info);
        }));

        let result = self.event_loop(&mut terminal);

        terminal::disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen, cursor::Show)?;
        terminal.show_cursor()?;
        result
    }

    fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> io::Result<bool> {
        while !self.should_quit {
            self.drain_events();
            terminal.draw(|f| self.draw(f))?;

            while event::poll(Duration::ZERO)? {
                let ev = event::read()?;
                self.on_event(ev);
                if self.should_quit {
                    break;
                }
            }
        }
        Ok(!self.session.devices().is_empty())
    }

    fn drain_events(&mut self) {
        let events = match &self.scanner {
            Some(scanner) => {
                let mut events = scanner.poll(Duration::from_millis(100));
                // 线程已结束时，队列里可能还有没取完的事件（一次 poll 最多 512 条）；
                // 必须在丢掉句柄前取干净，否则大批量应答的最后几台设备会丢。
                if scanner.is_finished() {
                    loop {
                        let more = scanner.poll(Duration::ZERO);
                        if more.is_empty() {
                            break;
                        }
                        events.extend(more);
                    }
                }
                events
            }
            None => {
                std::thread::sleep(Duration::from_millis(100));
                Vec::new()
            }
        };
        for ev in events {
            self.session.apply(&ev);
            if let ScanEvent::NetError(e) = &ev {
                self.set_status(tr_args("tui.status.error", &[("message", &e.to_string())]));
            }
        }
        if self
            .scanner
            .as_ref()
            .is_some_and(sadp_core::Scanner::is_finished)
        {
            // 单轮扫完是「结束」，不是「暂停」；否则空格会卡在一个无反应的暂停态。
            self.scanner = None;
            self.paused = false;
        }
    }

    /// 在状态栏显示一条 4 秒后自动消失的提示。
    pub fn set_status(&mut self, message: String) {
        self.status = Some((message, Instant::now()));
    }

    fn on_event(&mut self, ev: Event) {
        let Event::Key(key) = ev else { return };
        if key.kind != KeyEventKind::Press {
            return;
        }

        if self.mode == Mode::Filter {
            match key.code {
                KeyCode::Esc => {
                    self.filter_input.reset();
                    self.view.filter.clear();
                    self.mode = Mode::Normal;
                }
                KeyCode::Enter => {
                    self.view.filter = self.filter_input.value().to_string();
                    self.mode = Mode::Normal;
                }
                _ => {
                    if let Some(req) = to_input_request(&Event::Key(key)) {
                        self.filter_input.handle(req);
                        self.view.filter = self.filter_input.value().to_string();
                    }
                }
            }
            return;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Char('s') => self.start_scan(false),
            KeyCode::Char('r') => self.start_scan(true),
            KeyCode::Char(' ') => self.toggle_pause(),
            KeyCode::Char('/') => {
                self.mode = Mode::Filter;
                self.filter_input = Input::new(self.view.filter.clone());
            }
            // 不同终端对 Shift+L 的编码不一致：有的给 Char('L')，
            // 有的给 Char('l') + SHIFT；直接不按 Shift 按 l 也很常见。
            // 正常模式没有文本输入，接受两种大小写更稳。
            KeyCode::Char('L') => self.toggle_language(),
            KeyCode::Char('l') if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_language();
            }
            KeyCode::Char('e') => self.export(),
            KeyCode::Char('c') => self.copy_selected(),
            // J：打开日志目录。小写 j 仍是向下移动；部分终端把 Shift+J 报成
            // Char('j') + SHIFT，所以带 SHIFT 的小写 j 也认。
            KeyCode::Char('J') => self.open_logs(),
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::SHIFT) => self.open_logs(),
            KeyCode::Char('t') => {
                self.view.sort = self.view.sort.next();
                self.reselect();
            }
            KeyCode::Char('T') => {
                self.view.dir = self.view.dir.flip();
                self.reselect();
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::Home => self.select_index(0),
            KeyCode::End => {
                let last = visible_indices(self.session.devices(), &self.view).len();
                self.select_index(last.saturating_sub(1));
            }
            _ => {
                // 不同终端对 Shift/大小写的编码不一致；记下原始按键，
                // 用 `-vv` 跑一次就能知道某个键实际发的是什么。
                tracing::debug!(
                    code = ?key.code,
                    modifiers = ?key.modifiers,
                    "unhandled key"
                );
            }
        }
    }

    /// 启动一轮新的扫描线程。`clear` 为真时同时清空已有结果。
    fn start_scan(&mut self, clear: bool) {
        if let Some(mut scanner) = self.scanner.take() {
            scanner.stop();
        }
        if clear {
            self.session = Session::new();
            self.view = ViewOptions::default();
            self.table_state = TableState::default().with_selected(Some(0));
        }
        // 上一轮可能以 Finished 收尾，重新开始时必须复位，否则状态栏会一直显示「完成」。
        self.session.finished = false;
        self.session.last_error = None;

        match Scanner::start(self.cfg.clone(), Arc::clone(&self.transport)) {
            Ok(scanner) => {
                self.scanner = Some(scanner);
                self.paused = false;
            }
            Err(e) => {
                self.scanner = None;
                self.paused = true;
                self.set_status(tr_args("tui.status.error", &[("message", &e.to_string())]));
            }
        }
    }

    fn toggle_language(&mut self) {
        let lang = i18n::toggle();
        self.set_status(lang.native_name().to_string());
    }

    /// 空格 = 播放/暂停键：
    /// - 正在扫描 → 停掉扫描线程（立即冻结列表，而不是等当前轮跑完）；
    /// - 已暂停 → 重新开始扫描；
    /// - 上一轮已自然结束 → 视作继续扫描，而不是卡在一个按了没反应的「暂停」。
    fn toggle_pause(&mut self) {
        // 清掉导出/复制之类的临时提示，让状态栏立刻反映暂停/继续。
        self.status = None;
        if self.scanner.is_some() && !self.paused {
            if let Some(mut scanner) = self.scanner.take() {
                scanner.stop();
            }
            self.paused = true;
        } else {
            self.start_scan(false);
        }
    }

    fn move_selection(&mut self, delta: i32) {
        let len = visible_indices(self.session.devices(), &self.view).len();
        if len == 0 {
            self.table_state.select(None);
            return;
        }
        let current = self.table_state.selected().unwrap_or(0) as i32;
        let next = (current + delta).clamp(0, len as i32 - 1);
        self.table_state.select(Some(next as usize));
    }

    fn select_index(&mut self, index: usize) {
        let len = visible_indices(self.session.devices(), &self.view).len();
        if len == 0 {
            self.table_state.select(None);
        } else {
            self.table_state.select(Some(index.min(len - 1)));
        }
    }

    /// 排序/过滤变化后，尽量保持同一台设备仍被选中。
    fn reselect(&mut self) {
        let key = self.selected_device().map(|d| d.key());
        let visible = visible_indices(self.session.devices(), &self.view);
        let position = key.and_then(|k| {
            visible
                .iter()
                .position(|&i| self.session.devices()[i].key() == k)
        });
        self.select_index(position.unwrap_or(0));
    }

    fn selected_device(&self) -> Option<&Device> {
        let visible = visible_indices(self.session.devices(), &self.view);
        let index = self.table_state.selected()?;
        visible.get(index).map(|&i| &self.session.devices()[i])
    }

    fn export(&mut self) {
        let fmt = time::macros::format_description!("[year][month][day]-[hour][minute][second]");
        let stamp = time::OffsetDateTime::now_local()
            .unwrap_or_else(|_| time::OffsetDateTime::now_utc())
            .format(fmt)
            .unwrap_or_else(|_| "export".to_string());
        let path: PathBuf = format!("sadp-discover-{stamp}.json").into();
        let meta = Meta::from_session(&self.session, self.cfg.port, &self.cfg.families);
        match output::export_json(&path, &self.session, &meta) {
            Ok(()) => {
                let msg = tr_args("tui.export.done", &[("path", &path.display().to_string())]);
                self.set_status(msg);
            }
            Err(e) => {
                let msg = tr_args("tui.export.failed", &[("reason", &e.to_string())]);
                self.set_status(msg);
            }
        }
    }

    /// 打开日志目录，方便排障时一键跳转。
    fn open_logs(&mut self) {
        let Some(dir) = crate::logging::log_dir() else {
            self.set_status(tr("log.unavailable"));
            return;
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.set_status(tr_args("log.failed", &[("reason", &e.to_string())]));
            return;
        }
        let path = dir.display().to_string();
        match sadp_core::system::open_dir(&dir) {
            Ok(()) => self.set_status(tr_args("log.opened", &[("path", &path)])),
            Err(e) => self.set_status(tr_args("log.failed", &[("reason", &e.to_string())])),
        }
    }

    fn copy_selected(&mut self) {
        let Some(device) = self.selected_device() else {
            self.set_status(tr("tui.status.nothing_to_copy"));
            return;
        };
        let text = device_clipboard_text(device);
        match clipboard::copy(&text) {
            Ok(()) => self.set_status(tr("tui.status.copied")),
            Err(e) => self.set_status(tr_args("tui.status.error", &[("message", &e.to_string())])),
        }
    }

    fn draw(&mut self, f: &mut Frame) {
        let ifaces = self.transport.interfaces();
        let iface_text = match ifaces.len() {
            1 => tr_args("tui.header.iface", &[("iface", &ifaces[0].name)]),
            n => tr_args("tui.header.adapters", &[("count", &n.to_string())]),
        };
        let status = self.status_text();
        let filter_active = self.mode == Mode::Filter;
        let filter_query = self.filter_input.value().to_string();

        let mut state = RenderState {
            session: &self.session,
            view: &self.view,
            table: &mut self.table_state,
            paused: self.paused,
            filter_active,
            filter_query: &filter_query,
            status: &status,
            iface_label: &iface_text,
        };
        render(f, &mut state);
    }

    fn status_text(&self) -> String {
        if let Some((message, at)) = &self.status
            && at.elapsed() < STATUS_TTL
        {
            return message.clone();
        }
        let count = self.session.devices().len().to_string();
        if self.paused {
            return tr_args("tui.status.paused", &[("count", &count)]);
        }
        if self.session.finished {
            // 一台都没发现、而且一个 SADP 响应都没收到：多半是权限/防火墙，而不是"没设备"。
            if self.session.devices().is_empty()
                && self.session.sadp_packets == 0
                && !self.cfg.passive
            {
                return tr("tui.status.no_responses");
            }
            return tr_args(
                "tui.status.finished",
                &[
                    ("count", &count),
                    ("elapsed", &output::format_duration(self.session.elapsed())),
                ],
            );
        }
        if let Some(err) = &self.session.last_error {
            return tr_args("tui.status.error", &[("message", err)]);
        }
        if self.session.round == 0 {
            return tr_args("tui.status.waiting", &[("count", &count)]);
        }
        let mut text = tr_args(
            "tui.status.scanning",
            &[
                ("count", &count),
                ("round", &self.session.round.to_string()),
            ],
        );
        if !self.view.filter.is_empty() {
            let shown = visible_indices(self.session.devices(), &self.view).len();
            text.push_str("  ");
            text.push_str(&tr_args(
                "tui.status.filtered",
                &[("shown", &shown.to_string()), ("total", &count)],
            ));
        }
        text
    }
}

/// 渲染所需的全部状态。抽出来是为了让测试无需启动扫描线程。
pub struct RenderState<'a> {
    pub session: &'a Session,
    pub view: &'a ViewOptions,
    pub table: &'a mut TableState,
    pub paused: bool,
    pub filter_active: bool,
    pub filter_query: &'a str,
    pub status: &'a str,
    pub iface_label: &'a str,
}

pub fn render(f: &mut Frame, ui: &mut RenderState<'_>) {
    let area = f.area();
    if area.width < 40 || area.height < 8 {
        f.render_widget(Paragraph::new(tr("tui.empty")), area);
        return;
    }

    let detail_height = if area.height >= 26 { 8 } else { 5 };
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(detail_height),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    render_header(f, chunks[0], ui);
    render_table(f, chunks[1], ui);
    render_detail(f, chunks[2], ui);
    render_status(f, chunks[3], ui);
    f.render_widget(
        Paragraph::new(Span::styled(
            tr("tui.help.bar"),
            Style::new().fg(Color::DarkGray),
        )),
        chunks[4],
    );
}

fn render_header(f: &mut Frame, area: Rect, ui: &RenderState<'_>) {
    let devices = ui.session.devices().len();
    let line = Line::from(vec![
        Span::styled(
            tr("tui.title"),
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(ui.iface_label.to_string(), Style::new().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(
            tr_args(
                "tui.header.round",
                &[("round", &ui.session.round.to_string())],
            ),
            Style::new().fg(Color::DarkGray),
        ),
        Span::raw("  "),
        Span::styled(
            tr_args("tui.header.devices", &[("count", &devices.to_string())]),
            Style::new().fg(Color::DarkGray),
        ),
        Span::raw("  "),
        Span::styled(
            output::format_duration(ui.session.elapsed()),
            Style::new().fg(Color::DarkGray),
        ),
        if ui.paused {
            Span::styled(
                format!("  {}", tr("tui.header.paused")),
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::raw("")
        },
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn render_table(f: &mut Frame, area: Rect, ui: &mut RenderState<'_>) {
    let devices = ui.session.devices();
    let visible = visible_indices(devices, ui.view);
    if visible.is_empty() {
        let text = if ui.view.filter.is_empty() {
            tr("tui.empty")
        } else {
            tr_args(
                "tui.status.filtered",
                &[("shown", "0"), ("total", &devices.len().to_string())],
            )
        };
        f.render_widget(
            Paragraph::new(text).style(Style::new().fg(Color::DarkGray)),
            area,
        );
        ui.table.select(None);
        return;
    }

    let compact = area.width < 90;
    let headers: Vec<String> = if compact {
        vec![
            tr("tui.col.ip"),
            tr("tui.col.mac"),
            tr("tui.col.model"),
            tr("tui.col.via"),
        ]
    } else {
        vec![
            tr("tui.col.ip"),
            tr("tui.col.mac"),
            tr("tui.col.model"),
            tr("tui.col.sn"),
            tr("tui.col.via"),
            tr("tui.col.activated"),
        ]
    };
    let widths: Vec<Constraint> = if compact {
        vec![
            Constraint::Length(26),
            Constraint::Length(17),
            Constraint::Min(8),
            Constraint::Length(6),
        ]
    } else {
        vec![
            Constraint::Length(26),
            Constraint::Length(17),
            Constraint::Length(22),
            Constraint::Min(10),
            Constraint::Length(6),
            Constraint::Length(4),
        ]
    };

    let header = Row::new(headers.into_iter().map(Cell::from)).style(
        Style::new()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = visible
        .iter()
        .map(|&index| {
            let d = &devices[index];
            let via_style = match d.seen_via {
                sadp_core::SeenVia::V4 => Style::new().fg(Color::Green),
                sadp_core::SeenVia::V6 => Style::new().fg(Color::Magenta),
                sadp_core::SeenVia::Both => Style::new().fg(Color::Cyan),
            };
            let mut cells = vec![
                Cell::from(d.primary_ip()),
                Cell::from(d.mac.map(|m| m.to_string()).unwrap_or_default()),
                Cell::from(d.display_model().unwrap_or_default().to_string()),
            ];
            if !compact {
                cells.push(Cell::from(d.sn.clone().unwrap_or_default()));
            }
            cells.push(Cell::from(Span::styled(
                d.seen_via.as_str().to_string(),
                via_style,
            )));
            if !compact {
                cells.push(Cell::from(match d.activated {
                    Some(true) => "✓",
                    Some(false) => "✗",
                    None => "-",
                }));
            }
            Row::new(cells)
        })
        .collect();

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(Style::new().bg(Color::DarkGray))
        .highlight_symbol("› ");

    match ui.table.selected() {
        Some(selected) if selected >= visible.len() => {
            ui.table.select(Some(visible.len() - 1));
        }
        None => ui.table.select(Some(0)),
        _ => {}
    }

    f.render_stateful_widget(table, area, ui.table);
}

fn render_detail(f: &mut Frame, area: Rect, ui: &RenderState<'_>) {
    let block = Block::bordered()
        .title(Span::styled(
            tr("tui.detail.title"),
            Style::new().fg(Color::Cyan),
        ))
        .border_style(Style::new().fg(Color::DarkGray));

    let visible = visible_indices(ui.session.devices(), ui.view);
    let device = ui
        .table
        .selected()
        .and_then(|index| visible.get(index))
        .map(|&index| &ui.session.devices()[index]);

    let Some(device) = device else {
        f.render_widget(
            Paragraph::new(tr("tui.detail.empty"))
                .style(Style::new().fg(Color::DarkGray))
                .block(block),
            area,
        );
        return;
    };

    f.render_widget(Paragraph::new(detail_lines(device)).block(block), area);
}

fn render_status(f: &mut Frame, area: Rect, ui: &RenderState<'_>) {
    if ui.filter_active {
        let text = tr_args("tui.help.filter", &[("query", ui.filter_query)]);
        f.render_widget(
            Paragraph::new(Span::styled(
                text,
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )),
            area,
        );
        return;
    }
    f.render_widget(
        Paragraph::new(Span::styled(ui.status, Style::new().fg(Color::DarkGray))),
        area,
    );
}

fn detail_lines(d: &Device) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut first = Vec::new();
    if let Some(sn) = &d.sn {
        first.push(format!("{}: {sn}", tr("tui.detail.sn")));
    }
    if let Some(mac) = d.mac {
        first.push(format!("{}: {mac}", tr("tui.detail.mac")));
    }
    if let Some(model) = d.display_model() {
        first.push(format!("{}: {model}", tr("tui.detail.model")));
    }
    if !first.is_empty() {
        lines.push(Line::from(first.join("    ")));
    }

    let mut net = Vec::new();
    if let Some(ip) = d.ipv4 {
        let mask = d
            .netmask
            .map(|m| format!("/{}", mask_to_prefix(m)))
            .unwrap_or_default();
        net.push(format!("{}: {ip}{mask}", tr("tui.detail.ipv4")));
    }
    if let Some(gw) = d.gateway {
        net.push(format!("{}: {gw}", tr("tui.detail.gateway")));
    }
    if let Some(dhcp) = d.dhcp {
        net.push(format!(
            "{}: {}",
            tr("tui.detail.dhcp"),
            if dhcp {
                tr("common.yes")
            } else {
                tr("common.no")
            }
        ));
    }
    if !net.is_empty() {
        lines.push(Line::from(net.join("    ")));
    }

    if !d.ipv6.is_empty() {
        let addrs: Vec<String> = d
            .ipv6
            .iter()
            .map(|e| match e.scope_id {
                Some(scope) if !e.global => format!("{}%{scope}", e.addr),
                _ => e.addr.to_string(),
            })
            .collect();
        lines.push(Line::from(format!(
            "{}: {}",
            tr("tui.detail.ipv6"),
            addrs.join(", ")
        )));
    }

    let mut misc = Vec::new();
    if let Some(fw) = &d.firmware {
        misc.push(format!("{}: {fw}", tr("tui.detail.firmware")));
    }
    match (d.sdk_port, d.http_port) {
        (Some(sdk), Some(http)) => {
            misc.push(format!("{}: {sdk} / {http}", tr("tui.detail.ports")));
        }
        (Some(sdk), None) => misc.push(format!("{}: {sdk} / -", tr("tui.detail.ports"))),
        (None, Some(http)) => misc.push(format!("{}: - / {http}", tr("tui.detail.ports"))),
        (None, None) => {}
    }
    if let Some(activated) = d.activated {
        misc.push(format!(
            "{}: {}",
            tr("tui.detail.activated"),
            if activated {
                tr("common.yes")
            } else {
                tr("common.no")
            }
        ));
    }
    if !misc.is_empty() {
        lines.push(Line::from(misc.join("    ")));
    }

    let mut tail = vec![format!(
        "{}: {}",
        tr("tui.detail.first_seen"),
        output::format_duration(d.first_seen.elapsed())
    )];
    tail.push(format!(
        "{}: {}",
        tr("tui.detail.last_seen"),
        output::format_duration(d.last_seen.elapsed())
    ));
    if !d.seen_on.is_empty() {
        tail.push(format!(
            "{}: {}",
            tr("tui.detail.interfaces"),
            d.seen_on.join(", ")
        ));
    }
    tail.push(format!("{}: {}", tr("tui.detail.via"), d.seen_via.as_str()));
    lines.push(Line::from(tail.join("    ")));

    lines
}

fn mask_to_prefix(mask: std::net::Ipv4Addr) -> u8 {
    u32::from(mask).count_ones() as u8
}

fn device_clipboard_text(d: &Device) -> String {
    let mut out = String::new();
    if let Some(sn) = &d.sn {
        out.push_str(&format!("SN: {sn}\n"));
    }
    if let Some(mac) = d.mac {
        out.push_str(&format!("MAC: {mac}\n"));
    }
    if let Some(ip) = d.ipv4 {
        out.push_str(&format!("IPv4: {ip}\n"));
    }
    for ep in &d.ipv6 {
        match ep.scope_id {
            Some(scope) if !ep.global => {
                out.push_str(&format!("IPv6: {}%{scope}\n", ep.addr));
            }
            _ => out.push_str(&format!("IPv6: {}\n", ep.addr)),
        }
    }
    if let Some(model) = d.display_model() {
        out.push_str(&format!("Model: {model}\n"));
    }
    out.push_str(&format!("Via: {}\n", d.seen_via.as_str()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;
    use ratatui::backend::TestBackend;
    use std::sync::{Mutex, MutexGuard};
    use unicode_width::UnicodeWidthStr;

    /// locale 是进程级全局状态；涉及语言切换的测试必须串行。
    static LOCALE_LOCK: Mutex<()> = Mutex::new(());

    fn lock_locale() -> MutexGuard<'static, ()> {
        LOCALE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 宽字符在缓冲区里占两格，第二格是空格；直接拼接会出现「网 卡」。
    /// 断言前把空格去掉，避免把渲染实现细节当成语言问题。
    fn squash(text: &str) -> String {
        text.replace(' ', "")
    }

    fn render_app(app: &mut App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal.draw(|f| app.draw(f)).expect("draw");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }

    fn sample_device() -> Device {
        let mut d = Device::fresh();
        d.sn = Some("DS-2CD2T47G2-L20210618AAWRL12345678".into());
        d.mac = sadp_core::MacAddr::parse("a4:14:37:5f:4c:6d");
        d.ipv4 = Some("192.168.1.64".parse().unwrap());
        d.model = Some("DS-2CD2T47G2-L".into());
        d.firmware = Some("V5.7.3".into());
        d.activated = Some(true);
        d.seen_via = sadp_core::SeenVia::Both;
        d
    }

    fn render_to_string(
        session: &Session,
        lang: sadp_core::Lang,
        width: u16,
        height: u16,
    ) -> String {
        i18n::set(lang);
        let mut view = ViewOptions::default();
        let mut table = TableState::default();
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| {
                let mut state = RenderState {
                    session,
                    view: &mut view,
                    table: &mut table,
                    paused: false,
                    filter_active: false,
                    filter_query: "",
                    status: "status",
                    iface_label: "mock0",
                };
                render(f, &mut state);
            })
            .expect("draw");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }

    #[test]
    fn renders_device_and_help_in_both_languages() {
        let _guard = lock_locale();
        let mut session = Session::new();
        session.apply(&ScanEvent::RoundStarted {
            id: 1,
            at: Instant::now(),
        });
        session.apply(&ScanEvent::DeviceFound(sample_device()));

        for lang in sadp_core::Lang::ALL {
            let text = render_to_string(&session, lang, 80, 24);
            assert!(text.contains("192.168.1.64"), "缺少 IP: {lang:?}");
            assert!(text.contains("DS-2CD2T47G2-L"), "缺少型号: {lang:?}");
            assert!(text.contains("a4:14:37:5f:4c:6d"), "缺少 MAC: {lang:?}");
        }
        i18n::set(sadp_core::Lang::EnUs);
    }

    #[test]
    fn renders_empty_and_narrow_sizes_without_panic() {
        let _guard = lock_locale();
        let session = Session::new();
        let text = render_to_string(&session, sadp_core::Lang::EnUs, 80, 24);
        assert!(!text.is_empty());
        let text = render_to_string(&session, sadp_core::Lang::ZhCn, 40, 10);
        assert!(!text.is_empty());
        i18n::set(sadp_core::Lang::EnUs);
    }

    fn test_app(cfg: ScanConfig) -> App {
        let transport: Arc<dyn Transport> = Arc::new(sadp_net::MockTransport::single());
        App::new(cfg, transport).expect("app")
    }

    fn wait_until(mut cond: impl FnMut() -> bool, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        cond()
    }

    #[test]
    fn space_restarts_scan_after_single_round_finished() {
        let mut app = test_app(ScanConfig {
            timeout: Duration::from_millis(60),
            ..ScanConfig::default()
        });
        assert!(wait_until(
            || {
                app.drain_events();
                app.scanner.is_none()
            },
            Duration::from_secs(3)
        ));
        assert!(!app.paused, "自然结束不应被当成暂停");

        // 回归：单轮结束后按空格必须重新开始扫描，而不是原地无反应。
        app.toggle_pause();
        assert!(app.scanner.is_some(), "空格应重新开始扫描");
        assert!(!app.paused);
    }

    #[test]
    fn space_pauses_and_resumes_running_scan() {
        let mut app = test_app(ScanConfig {
            timeout: Duration::from_millis(40),
            repeat: Some(Duration::from_secs(5)),
            ..ScanConfig::default()
        });
        assert!(app.scanner.is_some());

        app.toggle_pause();
        assert!(app.paused);
        assert!(app.scanner.is_none(), "暂停应立即停掉扫描线程");
        let status = app.status_text();
        assert!(
            status.contains("暂停") || status.contains("Paused"),
            "status={status}"
        );

        app.toggle_pause();
        assert!(!app.paused);
        assert!(app.scanner.is_some());
    }

    #[test]
    fn rapid_space_presses_end_in_a_consistent_state() {
        let mut app = test_app(ScanConfig {
            timeout: Duration::from_millis(40),
            repeat: Some(Duration::from_secs(5)),
            ..ScanConfig::default()
        });
        for _ in 0..5 {
            app.toggle_pause();
        }
        assert!(app.paused, "奇数次切换后应为暂停");
        assert!(app.scanner.is_none());

        app.toggle_pause();
        assert!(!app.paused);
        assert!(app.scanner.is_some());
    }

    #[test]
    fn space_key_via_event_handler_toggles_pause() {
        let mut app = test_app(ScanConfig {
            timeout: Duration::from_millis(40),
            repeat: Some(Duration::from_secs(5)),
            ..ScanConfig::default()
        });
        let space = || Event::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));

        app.on_event(space());
        assert!(app.paused, "空格键应进入暂停");
        assert!(app.scanner.is_none());

        app.on_event(space());
        assert!(!app.paused, "再按空格应继续");
        assert!(app.scanner.is_some());
    }

    #[test]
    fn s_keeps_results_while_r_clears_them() {
        let mut app = test_app(ScanConfig {
            timeout: Duration::from_millis(40),
            repeat: Some(Duration::from_secs(5)),
            ..ScanConfig::default()
        });
        app.session.apply(&ScanEvent::DeviceFound(sample_device()));
        assert_eq!(app.session.devices().len(), 1);

        // s = 立即重扫，保留已发现设备
        app.on_event(Event::Key(KeyEvent::new(
            KeyCode::Char('s'),
            KeyModifiers::NONE,
        )));
        assert_eq!(app.session.devices().len(), 1, "s 不清空结果");
        assert!(app.scanner.is_some());

        // r = 清空后重扫
        app.on_event(Event::Key(KeyEvent::new(
            KeyCode::Char('r'),
            KeyModifiers::NONE,
        )));
        assert!(app.session.devices().is_empty(), "r 应清空结果");
        assert!(app.scanner.is_some());
    }

    #[test]
    fn start_scan_resets_finished_flag() {
        let mut app = test_app(ScanConfig {
            timeout: Duration::from_millis(50),
            ..ScanConfig::default()
        });
        assert!(wait_until(
            || {
                app.drain_events();
                app.scanner.is_none()
            },
            Duration::from_secs(3)
        ));
        assert!(app.session.finished);

        app.start_scan(false);
        assert!(!app.session.finished, "重扫后不能还显示「完成」");
        assert!(app.scanner.is_some());
    }

    #[test]
    fn help_bar_fits_eighty_columns() {
        let _guard = lock_locale();
        for lang in sadp_core::Lang::ALL {
            i18n::set(lang);
            let width = UnicodeWidthStr::width(tr("tui.help.bar").as_str());
            assert!(width <= 80, "{lang:?} 快捷键栏宽 {width} 列，超过 80 列");
        }
        i18n::set(sadp_core::Lang::EnUs);
    }

    #[test]
    fn language_toggle_repaints_the_ui() {
        let _guard = lock_locale();
        let mut app = test_app(ScanConfig {
            timeout: Duration::from_millis(40),
            repeat: Some(Duration::from_secs(5)),
            ..ScanConfig::default()
        });

        i18n::set(sadp_core::Lang::ZhCn);
        let before = render_app(&mut app, 100, 30);
        assert!(
            squash(&before).contains("网卡") || squash(&before).contains("设备"),
            "中文界面渲染异常: {before:?}"
        );

        app.on_event(Event::Key(KeyEvent::new(
            KeyCode::Char('L'),
            KeyModifiers::SHIFT,
        )));
        assert_eq!(i18n::current(), sadp_core::Lang::EnUs);

        let after = render_app(&mut app, 100, 30);
        assert!(
            squash(&after).contains("iface") || squash(&after).contains("device"),
            "切换语言后界面没有重绘成英文: {after:?}"
        );

        i18n::set(sadp_core::Lang::EnUs);
    }

    #[test]
    fn lowercase_l_also_toggles_language() {
        let _guard = lock_locale();
        let mut app = test_app(ScanConfig {
            timeout: Duration::from_millis(40),
            repeat: Some(Duration::from_secs(5)),
            ..ScanConfig::default()
        });

        i18n::set(sadp_core::Lang::EnUs);
        // 很多终端把 Shift+L 报成 Char('l') + SHIFT；也有用户直接按 l。
        app.on_event(Event::Key(KeyEvent::new(
            KeyCode::Char('l'),
            KeyModifiers::SHIFT,
        )));
        assert_eq!(i18n::current(), sadp_core::Lang::ZhCn);

        app.on_event(Event::Key(KeyEvent::new(
            KeyCode::Char('l'),
            KeyModifiers::NONE,
        )));
        assert_eq!(i18n::current(), sadp_core::Lang::EnUs);

        i18n::set(sadp_core::Lang::EnUs);
    }

    #[test]
    fn finished_with_zero_packets_shows_permission_hint() {
        let _guard = lock_locale();
        i18n::set(sadp_core::Lang::ZhCn);
        let mut app = test_app(ScanConfig {
            timeout: Duration::from_millis(40),
            ..ScanConfig::default()
        });

        app.session.finished = true;
        app.session.sadp_packets = 0;
        assert_eq!(app.status_text(), tr("tui.status.no_responses"));

        // 收到过响应就说明链路是通的，回到「完成」而不是权限提示。
        app.session.sadp_packets = 7;
        let status = app.status_text();
        assert!(
            status.contains("完成") || status.contains("Done"),
            "status={status}"
        );

        i18n::set(sadp_core::Lang::EnUs);
    }

    #[test]
    fn mask_prefix_conversion() {
        assert_eq!(mask_to_prefix("255.255.255.0".parse().unwrap()), 24);
        assert_eq!(mask_to_prefix("255.255.0.0".parse().unwrap()), 16);
        assert_eq!(mask_to_prefix("255.255.255.255".parse().unwrap()), 32);
    }
}
