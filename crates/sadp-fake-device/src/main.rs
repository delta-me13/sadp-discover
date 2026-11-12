//! 本地假 SADP 设备：加入组播、收到 Probe 后单播回 ProbeMatch。
//!
//! 用途：CI 与开发机上没有海康设备时，验证「组播发出 → 设备应答 → 解析 → 去重」
//! 整条链路（架构文档 §8 的 `fake-device`）。
//!
//! 与真实设备的差异：
//! - 报文由固定模板生成；
//! - 监听 37020（与真机一致）。同一台机器上同时跑扫描器时，请用
//!   `sadp-discover --local-port 0` 让扫描器绑定临时端口，避免抢同一个端口。
//! - 报文由模板生成，字段集合固定。

use std::io::Write;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, SockAddr, Socket, Type};

const MCAST_V4: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const MCAST_V6: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1);

struct Options {
    port: u16,
    families: Vec<&'static str>,
    duration: Duration,
    serial: String,
    mac: String,
    model: String,
}

fn main() {
    let opts = parse_args();
    println!(
        "fake-device: port={} families={:?} duration={:?} sn={}",
        opts.port, opts.families, opts.duration, opts.serial
    );
    println!("fake-device: ready");

    let replies = Arc::new(AtomicU64::new(0));
    let mut threads = Vec::new();

    if opts.families.contains(&"v4") {
        match bind_v4(opts.port) {
            Ok(socket) => {
                let socket = Arc::new(socket);
                let replies = Arc::clone(&replies);
                let opts = Arc::new(opts_snapshot(&opts));
                threads.push(thread::spawn(move || {
                    serve(socket, 4, replies, opts);
                }));
            }
            Err(e) => eprintln!("fake-device: IPv4 unavailable: {e}"),
        }
    }
    if opts.families.contains(&"v6") {
        match bind_v6(opts.port) {
            Ok(socket) => {
                let socket = Arc::new(socket);
                let replies = Arc::clone(&replies);
                let opts = Arc::new(opts_snapshot(&opts));
                threads.push(thread::spawn(move || {
                    serve(socket, 6, replies, opts);
                }));
            }
            Err(e) => eprintln!("fake-device: IPv6 unavailable: {e}"),
        }
    }

    if threads.is_empty() {
        eprintln!("fake-device: no socket could be bound; aborting");
        std::process::exit(2);
    }

    let deadline = Instant::now() + opts.duration;
    while Instant::now() < deadline {
        thread::sleep(Duration::from_millis(200));
    }
    println!(
        "fake-device: sent {} replies",
        replies.load(Ordering::Relaxed)
    );
}

fn opts_snapshot(o: &Options) -> Options {
    Options {
        port: o.port,
        families: vec![],
        duration: o.duration,
        serial: o.serial.clone(),
        mac: o.mac.clone(),
        model: o.model.clone(),
    }
}

fn parse_args() -> Options {
    let mut port = 37020u16;
    let mut duration = Duration::from_secs(30);
    let mut serial = "DS-2CD2T47G2-L20210618AAWRL12345678".to_string();
    let mut mac = "a4:14:37:5f:4c:6d".to_string();
    let mut model = "DS-2CD2T47G2-L".to_string();
    let mut families = vec!["v4", "v6"];

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--port" if i + 1 < args.len() => {
                port = args[i + 1].parse().unwrap_or(37020);
                i += 2;
            }
            "--duration" if i + 1 < args.len() => {
                let secs: u64 = args[i + 1].parse().unwrap_or(30);
                duration = Duration::from_secs(secs);
                i += 2;
            }
            "--serial" if i + 1 < args.len() => {
                serial = args[i + 1].clone();
                i += 2;
            }
            "--mac" if i + 1 < args.len() => {
                mac = args[i + 1].clone();
                i += 2;
            }
            "--model" if i + 1 < args.len() => {
                model = args[i + 1].clone();
                i += 2;
            }
            "--family" if i + 1 < args.len() => {
                families = match args[i + 1].as_str() {
                    "v4" => vec!["v4"],
                    "v6" => vec!["v6"],
                    _ => vec!["v4", "v6"],
                };
                i += 2;
            }
            "-h" | "--help" => {
                println!(
                    "usage: sadp-fake-device [--port N] [--family v4|v6|both] [--duration SECS]\n\
                     \x20                              [--serial SN] [--mac MAC] [--model MODEL]"
                );
                std::process::exit(0);
            }
            other => {
                eprintln!("fake-device: ignoring unknown argument {other}");
                i += 1;
            }
        }
    }

    Options {
        port,
        families,
        duration,
        serial,
        mac,
        model,
    }
}

fn bind_v4(port: u16) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_broadcast(true)?;
    socket.bind(&SockAddr::from(SocketAddr::from(([0, 0, 0, 0], port))))?;
    socket.set_read_timeout(Some(Duration::from_millis(300)))?;
    let _ = socket.set_multicast_loop_v4(true);

    let mut joined = 0;
    if let Ok(list) = if_addrs::get_if_addrs() {
        for entry in list {
            if let if_addrs::IfAddr::V4(v4) = entry.addr
                && socket.join_multicast_v4(&MCAST_V4, &v4.ip).is_ok()
            {
                joined += 1;
                println!(
                    "fake-device: joined {MCAST_V4} on {} ({})",
                    entry.name, v4.ip
                );
            }
        }
    }
    if joined == 0 {
        eprintln!("fake-device: warning: no IPv4 interface joined the multicast group");
    }
    Ok(socket.into())
}

fn bind_v6(port: u16) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_only_v6(true)?;
    socket.set_reuse_address(true)?;
    socket.bind(&SockAddr::from(SocketAddr::from(([0u16; 8], port))))?;
    socket.set_read_timeout(Some(Duration::from_millis(300)))?;
    let _ = socket.set_multicast_loop_v6(true);

    let mut joined = 0;
    if let Ok(list) = if_addrs::get_if_addrs() {
        for entry in list {
            if let if_addrs::IfAddr::V6(_) = entry.addr
                && let Some(index) = entry.index
                && socket.join_multicast_v6(&MCAST_V6, index).is_ok()
            {
                joined += 1;
                println!(
                    "fake-device: joined [{MCAST_V6}] on {} (if{index})",
                    entry.name
                );
            }
        }
    }
    if joined == 0 {
        eprintln!("fake-device: warning: no IPv6 interface joined the multicast group");
    }
    Ok(socket.into())
}

fn serve(socket: Arc<UdpSocket>, family: u8, replies: Arc<AtomicU64>, opts: Arc<Options>) {
    let mut buf = vec![0u8; 65_536];
    loop {
        match socket.recv_from(&mut buf) {
            Ok((len, src)) => {
                let payload = &buf[..len];
                if !payload.windows(7).any(|w| w == b"<Probe>") {
                    continue;
                }
                let reply = build_reply(family, &opts, src);
                match socket.send_to(reply.as_bytes(), src) {
                    Ok(_) => {
                        replies.fetch_add(1, Ordering::Relaxed);
                        println!("fake-device: replied to {src}");
                        let _ = std::io::stdout().flush();
                    }
                    Err(e) => eprintln!("fake-device: reply to {src} failed: {e}"),
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionRefused
                ) => {}
            Err(e) => {
                eprintln!("fake-device: recv failed: {e}");
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn build_reply(family: u8, opts: &Options, dst: SocketAddr) -> String {
    let uuid = uuid_like();
    let ipv6 = match dst {
        SocketAddr::V6(v6) => v6.ip().to_string(),
        _ => "fe80::21b:2bff:fe3c:4d5e".to_string(),
    };
    let (ipv4_block, ipv6_block) = if family == 4 {
        (
            "<IPv4Address>192.168.1.64</IPv4Address>\n  \
             <IPv4SubnetMask>255.255.255.0</IPv4SubnetMask>\n  \
             <IPv4Gateway>192.168.1.1</IPv4Gateway>\n  "
                .to_string(),
            String::new(),
        )
    } else {
        (
            "<IPv4Address>192.168.1.64</IPv4Address>\n  ".to_string(),
            format!("<IPv6Address>{ipv6}</IPv6Address>\n  "),
        )
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<ProbeMatch>\n  \
         <Uuid>{uuid}</Uuid>\n  <Types>inquiry</Types>\n  \
         <DeviceType>{model}</DeviceType>\n  \
         <DeviceDescription>fake-device</DeviceDescription>\n  \
         <DeviceSN>{serial}</DeviceSN>\n  \
         <CommandPort>8000</CommandPort>\n  <HttpPort>80</HttpPort>\n  \
         <MAC>{mac}</MAC>\n  {ipv4_block}{ipv6_block}\
         <DHCP>false</DHCP>\n  <DSPVersion>V5.7.3 build 210618</DSPVersion>\n  \
         <Activated>true</Activated>\n  <PasswordResetAbility>true</PasswordResetAbility>\n  \
         <AnalogChannelNum>0</AnalogChannelNum>\n  <DigitalChannelNum>1</DigitalChannelNum>\n  \
         <Support>0x00000000</Support>\n</ProbeMatch>\n",
        model = opts.model,
        serial = opts.serial,
        mac = opts.mac,
    )
}

fn uuid_like() -> String {
    // 不引入 uuid 依赖：用时间 + 进程 id 拼一个形似 UUID 的字符串即可。
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "{:08x}-{:04x}-4{:03x}-a{:03x}-{:012x}",
        (nanos >> 32) as u32,
        ((nanos >> 16) & 0xffff) as u16,
        (nanos & 0x0fff) as u16,
        ((nanos >> 12) & 0x0fff) as u16,
        nanos & 0xffff_ffff_ffff
    )
}
