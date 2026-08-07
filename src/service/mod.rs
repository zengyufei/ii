use crate::command::{
    DavArgs, DiscoverArgs, DropArgs, HealthArgs, HttpArgs, PacArgs, PasteArgs, PingArgs, PortArgs,
    ProxyArgs, QueueArgs, RecvArgs, SendArgs, Socks5Args, SpeedArgs, TunnelArgs, WakeArgs,
    WatchArgs, WebArgs, WebrtcArgs,
};
use anyhow::Result;

mod dav;
mod discover;
mod lan;
mod network;
mod queue;
mod recv;
mod send;
mod socks5;
mod tunnel;
mod watch;
mod web;
mod webrtc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferEvent {
    Started,
    TicketReady(String),
    Completed,
    Failed(String),
}

pub async fn send(args: SendArgs) -> Result<()> {
    send::run(args).await
}

pub async fn send_with_events(
    args: SendArgs,
    events: std::sync::mpsc::Sender<TransferEvent>,
) -> Result<()> {
    send::with_events(args, events).await
}

pub async fn queue(args: QueueArgs) -> Result<()> {
    queue::run(args).await
}

pub async fn watch(args: WatchArgs) -> Result<()> {
    watch::run(args).await
}

pub async fn web(args: WebArgs) -> Result<()> {
    web::run(args).await
}

pub async fn webrtc(args: WebrtcArgs) -> Result<()> {
    webrtc::run(args).await
}

pub async fn tunnel(args: TunnelArgs) -> Result<()> {
    tunnel::run(args).await
}

pub async fn recv(args: RecvArgs) -> Result<()> {
    recv::run(args).await
}

pub async fn discover(args: DiscoverArgs) -> Result<()> {
    discover::run(args).await
}

pub async fn dav(args: DavArgs) -> Result<()> {
    dav::run(args).await
}

pub async fn socks5(args: Socks5Args) -> Result<()> {
    socks5::run(args).await
}

pub async fn http(args: HttpArgs) -> Result<()> {
    lan::http(args).await
}

pub async fn paste(args: PasteArgs) -> Result<()> {
    lan::paste(args).await
}

pub async fn drop(args: DropArgs) -> Result<()> {
    lan::drop(args).await
}

pub async fn pac(args: PacArgs) -> Result<()> {
    lan::pac(args).await
}

pub async fn proxy(args: ProxyArgs) -> Result<()> {
    network::proxy(args).await
}

pub async fn tcp(args: crate::command::ForwardArgs) -> Result<()> {
    network::tcp(args).await
}

pub async fn udp(args: crate::command::ForwardArgs) -> Result<()> {
    network::udp(args).await
}

pub async fn ping(args: PingArgs) -> Result<()> {
    network::ping(args).await
}

pub async fn speed(args: SpeedArgs) -> Result<()> {
    match args {
        SpeedArgs::Serve { listen } => lan::speed_server(listen).await,
        SpeedArgs::Test { url, duration } => network::speed(url, duration).await,
    }
}

pub async fn wake(args: WakeArgs) -> Result<()> {
    network::wake(args).await
}

pub async fn port(args: PortArgs) -> Result<()> {
    network::port(args).await
}

pub async fn health(args: HealthArgs) -> Result<()> {
    network::health(args).await
}

pub async fn recv_with_events(
    args: RecvArgs,
    events: std::sync::mpsc::Sender<TransferEvent>,
) -> Result<()> {
    recv::with_events(args, events).await
}
