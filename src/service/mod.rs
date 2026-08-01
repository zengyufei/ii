use crate::command::{RecvArgs, SendArgs, TunnelArgs, WebArgs, WebrtcArgs};
use anyhow::Result;

mod recv;
mod send;
mod tunnel;
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

pub async fn recv_with_events(
    args: RecvArgs,
    events: std::sync::mpsc::Sender<TransferEvent>,
) -> Result<()> {
    recv::with_events(args, events).await
}
