//! 双栈 UDP 传输实现。
//!
//! 结构（对应架构文档 §6.4）：
//! - **每个地址族一个 socket**，绑本机 `37020`（失败退临时端口）；
//! - IPv4 socket 加入 `239.255.255.250`（逐网卡），发送时逐网卡设置 `IP_MULTICAST_IF`
//!   并追加子网定向广播；
//! - IPv6 socket 显式 `IPV6_V6ONLY`（否则会与 v4 抢同一端口），
//!   逐网卡 `join_multicast_v6`，发送时用 `IPV6_MULTICAST_IF` + scope id；
//! - 每个 socket 一个接收线程，读超时 200 ms，收到即丢进 channel，上层按 100 ms 节拍取。
//!
//! 之所以用「一族一 socket」而不是「一网卡一 socket」：绑定到具体网卡地址的 socket
//! 收不到广播与组播（Windows/POSIX 一致），而 `0.0.0.0` 上的多 socket 又会让
//! 单播回包归属不确定。逐网卡出口用 `IP_MULTICAST_IF` 控制即可，效果相同且更可靠。

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, TrySendError, bounded};
use socket2::{Domain, Protocol, SockAddr, SockRef, Socket, Type};
use tracing::{debug, warn};

use crate::iface::{InterfaceSelector, enumerate_interfaces};
use crate::{
    Datagram, Family, InterfaceInfo, MCAST_V4, MCAST_V6_DEFAULT, NetError, SADP_PORT, Transport,
};

/// 单次读取上限：SADP 报文 < 2 KB，64 KiB 只用于兜底与防伪造。
const MAX_DATAGRAM: usize = 64 * 1024;
/// 接收线程的读超时，决定关停响应的最坏延迟。
const RECV_TIMEOUT: Duration = Duration::from_millis(200);
/// 事件队列容量：1000 台设备 × 每轮 1 包，留足余量；满了丢包而不是阻塞接收线程。
const EVENT_QUEUE: usize = 4096;

/// 一条「组播加入」记录，供 `interfaces` 诊断命令展示。
#[derive(Clone, Debug, serde::Serialize)]
pub struct JoinStatus {
    pub iface: String,
    pub family: Family,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// 传输层配置。
#[derive(Clone, Debug)]
pub struct UdpConfig {
    pub port: u16,
    pub families: Vec<Family>,
    pub interfaces: InterfaceSelector,
    pub v6_mcast: Ipv6Addr,
    pub broadcast: bool,
    /// 绑定到指定 IPv4 地址；`None` = `0.0.0.0`。
    pub bind_v4: Option<Ipv4Addr>,
    /// 本地绑定端口；`None` = 与 `port` 相同，`Some(0)` = 临时端口。
    pub bind_port: Option<u16>,
    /// 37020 被占用时是否退到临时端口（官方 SADP 同时运行时需要）。
    pub allow_ephemeral_fallback: bool,
}

impl Default for UdpConfig {
    fn default() -> Self {
        Self {
            port: SADP_PORT,
            families: vec![Family::V4, Family::V6],
            interfaces: InterfaceSelector::Auto,
            v6_mcast: MCAST_V6_DEFAULT,
            broadcast: true,
            bind_v4: None,
            bind_port: None,
            allow_ephemeral_fallback: true,
        }
    }
}

/// 真实 UDP 传输。
pub struct UdpTransport {
    interfaces: Vec<InterfaceInfo>,
    v4: Option<Arc<UdpSocket>>,
    v6: Option<Arc<UdpSocket>>,
    port: u16,
    families: Vec<Family>,
    cfg: UdpConfig,
    rx: Receiver<Datagram>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    warnings: Vec<String>,
    joins: Vec<JoinStatus>,
}

impl UdpTransport {
    /// 创建并绑定双栈 socket。单族失败只降级该族；两族都失败才返回错误。
    pub fn bind(cfg: UdpConfig) -> Result<Self, NetError> {
        let interfaces = enumerate_interfaces(&cfg.interfaces)?;
        let mut warnings = Vec::new();

        let mut v4: Option<Arc<UdpSocket>> = None;
        let mut v6: Option<Arc<UdpSocket>> = None;
        let mut bound: Vec<Family> = Vec::new();
        let mut local_port = 0u16;
        let mut joins: Vec<JoinStatus> = Vec::new();

        if cfg.families.contains(&Family::V4) {
            if interfaces.iter().any(|i| i.ipv4.is_some()) {
                match Self::open_v4(&cfg, &interfaces, &mut warnings, &mut joins) {
                    Ok((sock, port)) => {
                        debug!(port, "ipv4 socket bound");
                        v4 = Some(Arc::new(sock));
                        bound.push(Family::V4);
                        local_port = port;
                    }
                    Err(e) => Self::degrade(&mut warnings, Family::V4, &e),
                }
            } else {
                Self::degrade_msg(&mut warnings, Family::V4, "no interface with IPv4 address");
            }
        }

        if cfg.families.contains(&Family::V6) {
            if interfaces.iter().any(|i| !i.ipv6.is_empty()) {
                match Self::open_v6(&cfg, &interfaces, &mut warnings, &mut joins) {
                    Ok((sock, port)) => {
                        debug!(port, "ipv6 socket bound");
                        if local_port == 0 {
                            local_port = port;
                        }
                        v6 = Some(Arc::new(sock));
                        bound.push(Family::V6);
                    }
                    Err(e) => Self::degrade(&mut warnings, Family::V6, &e),
                }
            } else {
                Self::degrade_msg(&mut warnings, Family::V6, "no interface with IPv6 address");
            }
        }

        if bound.is_empty() {
            return Err(NetError::NoInterface(format!(
                "cannot bind any address family (requested {:?})",
                cfg.families
            )));
        }

        let (tx, rx) = bounded(EVENT_QUEUE);
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();

        if let Some(sock) = &v4 {
            let local = sock
                .local_addr()
                .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], local_port)));
            threads.push(Self::spawn_receiver(
                Arc::clone(sock),
                Family::V4,
                interfaces.clone(),
                local,
                tx.clone(),
                Arc::clone(&stop),
            ));
        }
        if let Some(sock) = &v6 {
            let local = sock
                .local_addr()
                .unwrap_or_else(|_| SocketAddr::from(([0u16; 8], local_port)));
            threads.push(Self::spawn_receiver(
                Arc::clone(sock),
                Family::V6,
                interfaces.clone(),
                local,
                tx.clone(),
                Arc::clone(&stop),
            ));
        }
        drop(tx);

        Ok(Self {
            interfaces,
            v4,
            v6,
            port: local_port,
            families: bound,
            cfg,
            rx,
            stop,
            threads,
            warnings,
            joins,
        })
    }

    /// 绑定过程中的可读告警（端口回退、组播加入失败等），应展示给用户。
    #[must_use]
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// 每个网卡每个地址族的组播加入结果。
    #[must_use]
    pub fn joins(&self) -> &[JoinStatus] {
        &self.joins
    }

    fn degrade(warnings: &mut Vec<String>, family: Family, err: &NetError) {
        let msg = format!("{family} disabled: {err}");
        if err.is_family_unavailable() {
            debug!("{msg}");
        } else {
            warn!("{msg}");
        }
        warnings.push(msg);
    }

    fn degrade_msg(warnings: &mut Vec<String>, family: Family, reason: &str) {
        let msg = format!("{family} disabled: {reason}");
        warn!("{msg}");
        warnings.push(msg);
    }

    fn open_v4(
        cfg: &UdpConfig,
        interfaces: &[InterfaceInfo],
        warnings: &mut Vec<String>,
        joins: &mut Vec<JoinStatus>,
    ) -> Result<(UdpSocket, u16), NetError> {
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
            .map_err(|e| io_err("create ipv4 socket", e))?;
        socket
            .set_reuse_address(true)
            .map_err(|e| io_err("SO_REUSEADDR (v4)", e))?;
        socket
            .set_broadcast(true)
            .map_err(|e| io_err("SO_BROADCAST", e))?;

        let bind_ip = cfg.bind_v4.unwrap_or(Ipv4Addr::UNSPECIFIED);
        let bind_port = cfg.bind_port.unwrap_or(cfg.port);
        let port = bind_with_fallback(
            &socket,
            SocketAddr::from((bind_ip, bind_port)),
            cfg.allow_ephemeral_fallback,
            warnings,
        )?;

        socket
            .set_read_timeout(Some(RECV_TIMEOUT))
            .map_err(|e| io_err("SO_RCVTIMEO (v4)", e))?;
        let _ = socket.set_multicast_loop_v4(true);
        let _ = socket.set_multicast_ttl_v4(1);

        // 逐网卡加入组播组：只加默认网卡会漏掉多网卡/虚拟网卡上的设备。
        for iface in interfaces {
            let Some(ip) = iface.ipv4 else { continue };
            match socket.join_multicast_v4(&MCAST_V4, &ip) {
                Ok(()) => joins.push(JoinStatus {
                    iface: iface.name.clone(),
                    family: Family::V4,
                    ok: true,
                    detail: None,
                }),
                Err(e) => {
                    let msg = format!("join {MCAST_V4} on {} ({ip}) failed: {e}", iface.name);
                    warn!("{msg}");
                    warnings.push(msg);
                    joins.push(JoinStatus {
                        iface: iface.name.clone(),
                        family: Family::V4,
                        ok: false,
                        detail: Some(e.to_string()),
                    });
                }
            }
        }

        Ok((socket.into(), port))
    }

    fn open_v6(
        cfg: &UdpConfig,
        interfaces: &[InterfaceInfo],
        warnings: &mut Vec<String>,
        joins: &mut Vec<JoinStatus>,
    ) -> Result<(UdpSocket, u16), NetError> {
        let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))
            .map_err(|e| io_err("create ipv6 socket", e))?;
        // 必须显式 only_v6：否则 Windows 上会与 v4 的 37020 冲突（WSAEADDRINUSE）。
        socket
            .set_only_v6(true)
            .map_err(|e| io_err("IPV6_V6ONLY", e))?;
        socket
            .set_reuse_address(true)
            .map_err(|e| io_err("SO_REUSEADDR (v6)", e))?;

        let bind_port = cfg.bind_port.unwrap_or(cfg.port);
        let port = bind_with_fallback(
            &socket,
            SocketAddr::from(([0u16; 8], bind_port)),
            cfg.allow_ephemeral_fallback,
            warnings,
        )?;

        socket
            .set_read_timeout(Some(RECV_TIMEOUT))
            .map_err(|e| io_err("SO_RCVTIMEO (v6)", e))?;
        let _ = socket.set_multicast_loop_v6(true);
        let _ = socket.set_multicast_hops_v6(1);

        for iface in interfaces {
            if iface.ipv6.is_empty() {
                continue;
            }
            if iface.index == 0 {
                let msg = format!(
                    "skip IPv6 multicast join on {}: ifindex unknown",
                    iface.name
                );
                warnings.push(msg);
                joins.push(JoinStatus {
                    iface: iface.name.clone(),
                    family: Family::V6,
                    ok: false,
                    detail: Some("ifindex unknown".to_string()),
                });
                continue;
            }
            match socket.join_multicast_v6(&cfg.v6_mcast, iface.index) {
                Ok(()) => joins.push(JoinStatus {
                    iface: iface.name.clone(),
                    family: Family::V6,
                    ok: true,
                    detail: None,
                }),
                Err(e) => {
                    let msg = format!(
                        "join [{0}] on {1} (if{2}) failed: {e}",
                        cfg.v6_mcast, iface.name, iface.index
                    );
                    warn!("{msg}");
                    warnings.push(msg);
                    joins.push(JoinStatus {
                        iface: iface.name.clone(),
                        family: Family::V6,
                        ok: false,
                        detail: Some(e.to_string()),
                    });
                }
            }
        }

        Ok((socket.into(), port))
    }

    fn spawn_receiver(
        socket: Arc<UdpSocket>,
        family: Family,
        interfaces: Vec<InterfaceInfo>,
        local: SocketAddr,
        tx: Sender<Datagram>,
        stop: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        thread::Builder::new()
            .name(format!("sadp-recv-{family}"))
            .spawn(move || {
                let mut buf = vec![0u8; MAX_DATAGRAM];
                while !stop.load(Ordering::Relaxed) {
                    match socket.recv_from(&mut buf) {
                        Ok((len, src)) => {
                            let iface = attribute_interface(&interfaces, src, family);
                            let dg = Datagram {
                                bytes: buf[..len].to_vec(),
                                src,
                                iface,
                                family,
                                local,
                            };
                            match tx.try_send(dg) {
                                Ok(()) => {}
                                Err(TrySendError::Full(_)) => {
                                    debug!(family = %family, "event queue full, dropping datagram");
                                }
                                Err(TrySendError::Disconnected(_)) => break,
                            }
                        }
                        Err(e) => match e.kind() {
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => {}
                            io::ErrorKind::Interrupted => {}
                            // Windows 上收到 ICMP 端口不可达后，下一次 recv 会报 ConnectionReset；
                            // 这是常态（设备过滤单播），不能因此中断接收线程。
                            io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionRefused => {}
                            _ => {
                                warn!(family = %family, error = %e, "recv failed");
                                thread::sleep(Duration::from_millis(50));
                            }
                        },
                    }
                }
            })
            .expect("spawn receiver thread")
    }

    fn send_v4(&self, sock: &UdpSocket, payload: &[u8]) -> Result<usize, NetError> {
        let mut sent = 0usize;
        let mut last_err: Option<NetError> = None;
        for iface in &self.interfaces {
            let Some(ip) = iface.ipv4 else { continue };
            let res = (|| -> io::Result<()> {
                let sref = SockRef::from(sock);
                sref.set_multicast_if_v4(&ip)?;
                sock.send_to(payload, SocketAddr::from((MCAST_V4, self.cfg.port)))?;
                if self.cfg.broadcast
                    && let Some(b) = iface.broadcast
                {
                    // 子网定向广播能保证从该网卡出去；255.255.255.255 只走默认路由。
                    sock.send_to(payload, SocketAddr::from((b, self.cfg.port)))?;
                }
                Ok(())
            })();
            match res {
                Ok(()) => sent += 1,
                Err(e) => {
                    debug!(iface = %iface.name, error = %e, "ipv4 probe send failed");
                    last_err = Some(io_err(&format!("send v4 via {}", iface.name), e));
                }
            }
        }
        if sent == 0 {
            return Err(last_err.unwrap_or_else(|| NetError::NoInterface("IPv4".into())));
        }
        Ok(sent)
    }

    fn send_v6(&self, sock: &UdpSocket, payload: &[u8]) -> Result<usize, NetError> {
        let mut sent = 0usize;
        let mut last_err: Option<NetError> = None;
        for iface in &self.interfaces {
            if iface.ipv6.is_empty() || iface.index == 0 {
                continue;
            }
            let res = (|| -> io::Result<()> {
                let sref = SockRef::from(sock);
                sref.set_multicast_if_v6(iface.index)?;
                let dest = SocketAddrV6::new(self.cfg.v6_mcast, self.cfg.port, 0, iface.index);
                sock.send_to(payload, SocketAddr::V6(dest))?;
                Ok(())
            })();
            match res {
                Ok(()) => sent += 1,
                Err(e) => {
                    debug!(iface = %iface.name, error = %e, "ipv6 probe send failed");
                    last_err = Some(io_err(&format!("send v6 via {}", iface.name), e));
                }
            }
        }
        if sent == 0 {
            return Err(last_err.unwrap_or_else(|| NetError::NoInterface("IPv6".into())));
        }
        Ok(sent)
    }
}

impl Transport for UdpTransport {
    fn interfaces(&self) -> &[InterfaceInfo] {
        &self.interfaces
    }

    fn send_probe(&self, families: &[Family], payload: &[u8]) -> Result<(), NetError> {
        let mut sent_total = 0usize;
        let mut last_err: Option<NetError> = None;

        for family in families {
            match (family, &self.v4, &self.v6) {
                (Family::V4, Some(sock), _) => match self.send_v4(sock, payload) {
                    Ok(n) => sent_total += n,
                    Err(e) => last_err = Some(e),
                },
                (Family::V6, _, Some(sock)) => match self.send_v6(sock, payload) {
                    Ok(n) => sent_total += n,
                    Err(e) => last_err = Some(e),
                },
                _ => {}
            }
        }

        if sent_total == 0 {
            return Err(last_err.unwrap_or_else(|| {
                NetError::NoInterface("no bound family matches requested".into())
            }));
        }
        Ok(())
    }

    fn recv(&self, timeout: Duration) -> Result<Option<Datagram>, NetError> {
        match self.rx.recv_timeout(timeout) {
            Ok(dg) => Ok(Some(dg)),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => Ok(None),
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => Ok(None),
        }
    }

    fn local_port(&self) -> u16 {
        self.port
    }

    fn bound_families(&self) -> &[Family] {
        &self.families
    }
}

impl Drop for UdpTransport {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for handle in self.threads.drain(..) {
            let _ = handle.join();
        }
    }
}

/// 绑定端口；被占用时按配置退到临时端口并记录告警。
fn bind_with_fallback(
    socket: &Socket,
    addr: SocketAddr,
    allow_fallback: bool,
    warnings: &mut Vec<String>,
) -> Result<u16, NetError> {
    match socket.bind(&SockAddr::from(addr)) {
        Ok(()) => Ok(socket
            .local_addr()
            .ok()
            .and_then(|a| a.as_socket())
            .map_or(addr.port(), |a| a.port())),
        Err(e) if allow_fallback && addr.port() != 0 => {
            let msg = format!("bind {addr} failed ({e}); falling back to ephemeral port");
            warn!("{msg}");
            warnings.push(msg);
            let fallback = SocketAddr::new(addr.ip(), 0);
            socket
                .bind(&SockAddr::from(fallback))
                .map_err(|e2| io_err(&format!("bind {fallback}"), e2))?;
            let port = socket
                .local_addr()
                .ok()
                .and_then(|a| a.as_socket())
                .map_or(0, |a| a.port());
            Ok(port)
        }
        Err(e) => Err(io_err(&format!("bind {addr}"), e)),
    }
}

/// 把回包归属到网卡：v4 按子网匹配，v6 按 scope_id / 地址匹配。
fn attribute_interface(
    interfaces: &[InterfaceInfo],
    src: SocketAddr,
    family: Family,
) -> InterfaceInfo {
    let fallback = || {
        interfaces
            .iter()
            .find(|i| !i.loopback)
            .or_else(|| interfaces.first())
            .cloned()
            .unwrap_or(InterfaceInfo {
                name: "<unknown>".into(),
                index: 0,
                ipv4: None,
                netmask: None,
                broadcast: None,
                ipv6: vec![],
                loopback: false,
            })
    };

    match (family, src) {
        (Family::V4, SocketAddr::V4(v4)) => interfaces
            .iter()
            .find(|i| i.contains_v4(*v4.ip()))
            .or_else(|| interfaces.iter().find(|i| i.ipv4 == Some(*v4.ip())))
            .cloned()
            .unwrap_or_else(fallback),
        (Family::V6, SocketAddr::V6(v6)) => {
            let scope = v6.scope_id();
            if scope != 0
                && let Some(i) = interfaces.iter().find(|i| i.index == scope)
            {
                return i.clone();
            }
            interfaces
                .iter()
                .find(|i| i.ipv6.contains(v6.ip()))
                .cloned()
                .unwrap_or_else(fallback)
        }
        _ => fallback(),
    }
}

fn io_err(context: &str, e: io::Error) -> NetError {
    let code = e.raw_os_error();
    match code {
        Some(code) => NetError::Io(format!("{context}: {e} (os error {code})")),
        None => NetError::Io(format!("{context}: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iface(name: &str, index: u32, ip: &str, mask: &str) -> InterfaceInfo {
        InterfaceInfo {
            name: name.into(),
            index,
            ipv4: Some(ip.parse().unwrap()),
            netmask: Some(mask.parse().unwrap()),
            broadcast: None,
            ipv6: vec![],
            loopback: false,
        }
    }

    #[test]
    fn attributes_datagram_to_subnet_interface() {
        let a = iface("eth0", 3, "192.168.1.10", "255.255.255.0");
        let b = iface("eth1", 4, "10.0.0.5", "255.255.255.0");
        let ifaces = vec![a.clone(), b.clone()];
        let src: SocketAddr = "10.0.0.77:37020".parse().unwrap();
        assert_eq!(attribute_interface(&ifaces, src, Family::V4).name, "eth1");
        let src: SocketAddr = "192.168.1.200:37020".parse().unwrap();
        assert_eq!(attribute_interface(&ifaces, src, Family::V4).name, "eth0");
    }

    #[test]
    fn attributes_ipv6_by_scope() {
        let mut a = iface("eth0", 3, "192.168.1.10", "255.255.255.0");
        a.ipv6 = vec!["fe80::1".parse().unwrap()];
        let mut b = iface("eth1", 4, "10.0.0.5", "255.255.255.0");
        b.ipv6 = vec!["fe80::2".parse().unwrap()];
        let ifaces = vec![a, b];
        let src = SocketAddr::V6(SocketAddrV6::new("fe80::a".parse().unwrap(), 37020, 0, 4));
        assert_eq!(attribute_interface(&ifaces, src, Family::V6).name, "eth1");
    }
}
