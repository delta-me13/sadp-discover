//! 扫描会话：后台线程跑「发包 → 收包 → 解码 → 合并」循环，前台消费事件。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError};
use sadp_net::{Family, NetError, Transport};
use tracing::{debug, warn};

use super::config::{ConfigError, ScanConfig};
use super::event::ScanEvent;
use super::store::{DeviceStore, Upsert};
use crate::proto::{Device, SeenVia, decode_probe_match, encode_probe, new_probe_uuid};

const EVENT_QUEUE: usize = 4096;
/// 单次 `poll` 最多返回的事件数，防止包风暴把一帧 UI 卡住。
const MAX_DRAIN: usize = 512;

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("failed to spawn scanner thread: {0}")]
    Spawn(String),
    #[error(transparent)]
    Net(#[from] NetError),
}

/// 扫描器句柄。线程在 `Drop` 时被要求退出并 join。
pub struct Scanner {
    rx: Receiver<ScanEvent>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Scanner {
    pub fn start(cfg: ScanConfig, transport: Arc<dyn Transport>) -> Result<Self, CoreError> {
        cfg.validate()?;
        let (tx, rx) = crossbeam_channel::bounded(EVENT_QUEUE);
        let stop = Arc::new(AtomicBool::new(false));

        let thread_stop = Arc::clone(&stop);
        let handle = thread::Builder::new()
            .name("sadp-scan".into())
            .spawn(move || run(cfg, transport, tx, thread_stop))
            .map_err(|e| CoreError::Spawn(e.to_string()))?;

        Ok(Self {
            rx,
            stop,
            handle: Some(handle),
        })
    }

    /// 阻塞至多 `timeout` 取事件；有事件时把队列里已有的也一并取走（上限 512）。
    pub fn poll(&self, timeout: Duration) -> Vec<ScanEvent> {
        let mut out = Vec::new();
        match self.rx.recv_timeout(timeout) {
            Ok(ev) => out.push(ev),
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return out,
        }
        while out.len() < MAX_DRAIN {
            match self.rx.try_recv() {
                Ok(ev) => out.push(ev),
                Err(_) => break,
            }
        }
        out
    }

    /// 请求停止并等待线程退出。
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.handle.as_ref().is_none_or(JoinHandle::is_finished)
    }
}

impl Drop for Scanner {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 前台状态：TUI 与 `--json` 共用，避免两套聚合逻辑。
#[derive(Debug, Default)]
pub struct Session {
    pub store: DeviceStore,
    pub round: u64,
    pub round_found: usize,
    pub last_round_elapsed: Duration,
    pub decode_failures: u64,
    /// 累计收到的 SADP 响应报文数（含解析失败的）。
    pub sadp_packets: usize,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub last_error: Option<String>,
    pub finished: bool,
    pub started_at: Option<Instant>,
    pub last_event_at: Option<Instant>,
}

impl Session {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, event: &ScanEvent) {
        self.last_event_at = Some(Instant::now());
        match event {
            ScanEvent::RoundStarted { id, at } => {
                self.round = *id;
                self.started_at.get_or_insert(*at);
            }
            ScanEvent::DeviceFound(device) | ScanEvent::DeviceUpdated { device, .. } => {
                // 引擎已合并过一次；这里再走一遍 upsert 是幂等的，
                // 好处是 `--json` 与 TUI 都只依赖事件流，不共享内部状态。
                let _ = self.store.upsert(device.clone());
            }
            ScanEvent::DeviceExpired { key } => {
                self.store.remove(key);
            }
            ScanEvent::DecodeFailed { src, error } => {
                self.decode_failures += 1;
                debug!(%src, error, "decode failed");
            }
            ScanEvent::NetError(e) => {
                let msg = e.to_string();
                warn!("{msg}");
                self.errors.push(msg.clone());
                self.last_error = Some(msg);
            }
            ScanEvent::RoundFinished {
                id,
                found,
                elapsed,
                sadp_packets,
            } => {
                self.round = *id;
                self.round_found = *found;
                self.last_round_elapsed = *elapsed;
                self.sadp_packets = *sadp_packets;
            }
            ScanEvent::Stopped => self.finished = true,
        }
    }

    #[must_use]
    pub fn devices(&self) -> &[Device] {
        self.store.devices()
    }

    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.started_at.map_or(Duration::ZERO, |t| t.elapsed())
    }
}

/// 非交互模式：跑到单轮结束（或连续模式被外部停止）后返回最终会话。
pub fn run_blocking(cfg: ScanConfig, transport: Arc<dyn Transport>) -> Result<Session, CoreError> {
    let scanner = Scanner::start(cfg, transport)?;
    let mut session = Session::new();
    loop {
        let events = scanner.poll(Duration::from_millis(100));
        for ev in &events {
            session.apply(ev);
        }
        if session.finished || scanner.is_finished() {
            // 把停止前残留的事件取干净。
            for ev in scanner.poll(Duration::ZERO) {
                session.apply(&ev);
            }
            break;
        }
    }
    Ok(session)
}

fn run(
    cfg: ScanConfig,
    transport: Arc<dyn Transport>,
    tx: crossbeam_channel::Sender<ScanEvent>,
    stop: Arc<AtomicBool>,
) {
    let mut store = DeviceStore::new();
    let mut round: u64 = 0;
    let mut sadp_packets: usize = 0;

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        round += 1;
        let started = Instant::now();
        if tx
            .send(ScanEvent::RoundStarted {
                id: round,
                at: started,
            })
            .is_err()
        {
            return;
        }

        if !cfg.passive {
            let uuid = new_probe_uuid();
            let payload = encode_probe(&uuid);
            if let Err(e) = transport.send_probe(&cfg.families, &payload) {
                warn!(error = %e, "probe send failed");
                if tx.send(ScanEvent::NetError(e)).is_err() {
                    return;
                }
            }
        }

        let deadline = started + cfg.timeout;
        while !stop.load(Ordering::Relaxed) {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let wait = (deadline - now).min(Duration::from_millis(100));
            match transport.recv(wait) {
                Ok(Some(dg)) => {
                    if crate::proto::looks_like_probe_match(&dg.bytes) {
                        sadp_packets += 1;
                    }
                    if let Some(dir) = cfg.save_raw.as_deref() {
                        save_raw(dir, &dg);
                    }
                    if let Some(event) = handle_datagram(&mut store, dg)
                        && tx.send(event).is_err()
                    {
                        return;
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    if tx.send(ScanEvent::NetError(e)).is_err() {
                        return;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
            }
        }

        if let Some(stale) = cfg.stale_after {
            for key in store.prune(stale) {
                if tx.send(ScanEvent::DeviceExpired { key }).is_err() {
                    return;
                }
            }
        }

        if tx
            .send(ScanEvent::RoundFinished {
                id: round,
                found: store.len(),
                elapsed: started.elapsed(),
                sadp_packets,
            })
            .is_err()
        {
            return;
        }
        debug!(round, found = store.len(), "round finished");

        let Some(period) = cfg.repeat else { break };
        let wait = period.saturating_sub(started.elapsed());
        if !sleep_interruptible(wait, &stop) {
            break;
        }
    }

    let _ = tx.send(ScanEvent::Stopped);
}

/// 处理一条数据报，返回要发给 UI 的事件；无变化时返回 `None`（避免 UI 空转重绘）。
fn handle_datagram(store: &mut DeviceStore, dg: sadp_net::Datagram) -> Option<ScanEvent> {
    let now = Instant::now();
    match decode_probe_match(&dg.bytes) {
        Ok(pm) => {
            let mut device = pm.device;
            device.seen_via = match dg.family {
                Family::V4 => SeenVia::V4,
                Family::V6 => SeenVia::V6,
            };
            device.seen_on = vec![dg.iface.name.clone()];
            device.first_seen = now;
            device.last_seen = now;

            // 链路本地 IPv6 缺 scope 就没法连接/展示，用来源网卡补上。
            if dg.family == Family::V6 {
                for ep in &mut device.ipv6 {
                    if !ep.global && ep.scope_id.is_none() {
                        ep.scope_id = Some(dg.iface.index);
                    }
                }
            }

            match store.upsert(device) {
                Upsert::Inserted(d) => Some(ScanEvent::DeviceFound(d)),
                Upsert::Updated {
                    key,
                    changed,
                    device,
                } => Some(ScanEvent::DeviceUpdated {
                    key,
                    changed,
                    device,
                }),
                Upsert::Unchanged {
                    key,
                    device,
                    refreshed,
                } => refreshed.then(|| ScanEvent::DeviceUpdated {
                    key,
                    changed: Vec::new(),
                    device,
                }),
                Upsert::Ignored => None,
            }
        }
        Err(e) => Some(ScanEvent::DecodeFailed {
            src: dg.src,
            error: e.to_string(),
        }),
    }
}

/// 原始报文落盘：`raw-<毫秒时间戳>-<源地址>.xml`。
fn save_raw(dir: &std::path::Path, dg: &sadp_net::Datagram) {
    // 组播回环会把自己发出的 Probe 也收回来；那不是设备应答，别写进目录。
    if dg.bytes.windows(7).any(|w| w == b"<Probe>") {
        return;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let name = format!("raw-{stamp}-{}.xml", dg.src.ip());
    let path = dir.join(name.replace(':', "_"));
    if let Err(e) = std::fs::write(&path, &dg.bytes) {
        warn!(path = %path.display(), error = %e, "cannot save raw datagram");
    }
}

fn sleep_interruptible(dur: Duration, stop: &AtomicBool) -> bool {
    let deadline = Instant::now() + dur;
    loop {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        thread::sleep((deadline - now).min(Duration::from_millis(50)));
    }
}

/// 供测试直接驱动合并逻辑（不启动线程）。
#[doc(hidden)]
pub fn apply_datagram(store: &mut DeviceStore, dg: sadp_net::Datagram) -> Option<ScanEvent> {
    handle_datagram(store, dg)
}
