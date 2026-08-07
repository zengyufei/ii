use crate::{
    command::RecvArgs,
    service::TransferEvent,
    ticket::{PayloadKind, ResumeRequest, Ticket},
    transport::{
        archive::extract_tar_stream,
        iroh::{EndpointPolicy, FILE_ALPN, bind_endpoint},
        p2p::{
            FilePlan, RecvTrace, connect_to_peer, copy_to_stdout, filter_local_addrs,
            payload_kind_name, plan_file_receive, relay_only_addr, trace_endpoint_addr,
            write_to_file,
        },
        progress::should_show_progress,
    },
};
use anyhow::{Context, Result, bail};
use iroh::RelayMode;

pub(super) async fn run(args: RecvArgs) -> Result<()> {
    run_impl(args).await
}

pub(super) async fn with_events(
    args: RecvArgs,
    events: std::sync::mpsc::Sender<TransferEvent>,
) -> Result<()> {
    with_events_impl(args, events).await
}

async fn run_impl(args: RecvArgs) -> Result<()> {
    let json = args.json;
    if json {
        crate::json::started("recv");
    }
    let mut trace = RecvTrace::new(args.trace);
    let show_progress = !json && should_show_progress(args.trace);
    if json {
        crate::json::progress("recv", 0);
    }
    trace.info(format_args!(
        "mode: {}",
        if args.local {
            "local-only"
        } else {
            "default relay path"
        }
    ));

    let ticket = Ticket::decode(&args.ticket)?;
    if args.quic_port.is_some()
        && (ticket.s3_route().is_some()
            || ticket.webdav_route().is_some()
            || ticket.ftp_route().is_some()
            || ticket.sftp_route().is_some())
    {
        bail!("--quic-port requires a P2P ticket");
    }
    if ticket.tunnel_route().is_some() {
        bail!("tunnel tickets require ii tunnel -c <ticket>");
    }
    trace.step("decode ticket");
    trace.info(format_args!(
        "ticket: kind={}, name={}, size={}",
        payload_kind_name(ticket.kind()),
        ticket.name(),
        ticket
            .size()
            .map(|size| size.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    ));
    if let Some(endpoint) = ticket.endpoint() {
        trace_endpoint_addr("ticket endpoints", endpoint, &trace);
    }
    if let Some(s3) = ticket.s3_route() {
        trace.info(format_args!(
            "ticket object-storage object: {}",
            s3.object_key
        ));
    }

    let out_dir = args
        .out_dir
        .clone()
        .unwrap_or(std::env::current_dir().context("current dir")?);
    let file_target = if matches!(ticket.kind(), PayloadKind::File | PayloadKind::Stdin)
        && !args.stdout
    {
        let path = out_dir.join(ticket.name());
        let plan = plan_file_receive(&args, &ticket, &path, &trace).await?;
        if plan == FilePlan::Skip {
            trace.info(format_args!("skipped identical file {}", path.display()));
            if json {
                crate::json::emit(
                    "skipped",
                    &[("path", crate::json::Value::String(&path.to_string_lossy()))],
                );
            } else {
                eprintln!("ii recv: skipped identical file {}", path.display());
            }
            if let Some(s3) = ticket.s3_route() {
                crate::backend::s3::try_delete_s3(s3.delete_url.clone(), &mut trace).await;
            }
            if let Some(webdav) = ticket.webdav_route() {
                crate::backend::webdav::try_delete_webdav_for_ticket(
                    webdav.clone(),
                    &mut trace,
                    json,
                )
                .await;
            }
            if let Some(ftp) = ticket.ftp_route() {
                crate::backend::ftp::try_delete_ftp_for_ticket(ftp.clone(), &mut trace, json).await;
            }
            if let Some(sftp) = ticket.sftp_route() {
                crate::backend::sftp::try_delete_sftp_for_ticket(sftp.clone(), &mut trace, json)
                    .await;
            }
            if json {
                crate::json::completed("recv");
            }
            return Ok(());
        }
        Some((path, plan))
    } else {
        None
    };

    if ticket.s3_route().is_some() {
        let result =
            crate::backend::s3::recv_s3(args, ticket, out_dir, file_target, trace, show_progress)
                .await;
        if result.is_ok() && json {
            crate::json::completed("recv");
        }
        return result;
    }
    if ticket.webdav_route().is_some() {
        let result = crate::backend::webdav::recv_webdav(
            args,
            ticket,
            out_dir,
            file_target,
            trace,
            show_progress,
        )
        .await;
        if result.is_ok() && json {
            crate::json::completed("recv");
        }
        return result;
    }
    if ticket.ftp_route().is_some() {
        let result =
            crate::backend::ftp::recv_ftp(args, ticket, out_dir, file_target, trace, show_progress)
                .await;
        if result.is_ok() && json {
            crate::json::completed("recv");
        }
        return result;
    }
    if ticket.sftp_route().is_some() {
        let result = crate::backend::sftp::recv_sftp(
            args,
            ticket,
            out_dir,
            file_target,
            trace,
            show_progress,
        )
        .await;
        if result.is_ok() && json {
            crate::json::completed("recv");
        }
        return result;
    }

    let relay_only = ticket.is_relay_only();
    if relay_only && args.local {
        bail!("--local cannot be used with a relay-only ticket");
    }
    let policy = if relay_only {
        let relay_url = ticket
            .endpoint()
            .and_then(|endpoint| endpoint.relay_urls().next())
            .cloned()
            .context("relay-only ticket is missing its relay URL")?;
        if ticket.is_self_signed_relay_only() {
            EndpointPolicy::SelfSignedRelayOnly(relay_url)
        } else {
            EndpointPolicy::TrustedRelayOnly(relay_url)
        }
    } else if args.local {
        EndpointPolicy::standard(RelayMode::Disabled)
    } else {
        EndpointPolicy::standard(RelayMode::Default)
    };
    let endpoint = bind_endpoint(policy, FILE_ALPN, args.quic_port).await?;
    trace.step("bind endpoint");
    if !args.local {
        trace.info("waiting for endpoint to go online");
        endpoint.online().await;
        trace.step("wait online");
    }

    let mut endpoint_addr = ticket
        .endpoint()
        .cloned()
        .context("peer ticket missing endpoint")?;
    if relay_only {
        endpoint_addr =
            relay_only_addr(&endpoint_addr).context("relay-only ticket has no relay address")?;
        trace.info(if ticket.is_self_signed_relay_only() {
            "using self-signed relay-only path"
        } else {
            "using verified relay-only path"
        });
        trace_endpoint_addr("relay-only endpoints", &endpoint_addr, &trace);
    } else if args.local {
        endpoint_addr = filter_local_addrs(endpoint_addr);
        trace_endpoint_addr("local-filtered endpoints", &endpoint_addr, &trace);
    }
    if endpoint_addr.addrs.is_empty() {
        bail!("ticket has no usable addresses for this mode");
    }

    let conn = connect_to_peer(
        &endpoint,
        endpoint_addr,
        args.local || relay_only,
        FILE_ALPN,
        &trace,
    )
    .await?;
    trace.step("connect to sender");

    let (mut send, recv) = conn.open_bi().await.context("open transfer stream")?;
    trace.step("open transfer stream");

    let resume_from = file_target
        .as_ref()
        .map(|(_, plan)| match plan {
            FilePlan::Download { resume_from } => *resume_from,
            FilePlan::Skip => 0,
        })
        .unwrap_or(0);
    if resume_from > 0 {
        trace.info(format_args!("resume from byte {}", resume_from));
    }
    let request = ResumeRequest { resume_from };
    let request_bytes = postcard::to_stdvec(&request).context("encode resume request")?;
    send.write_all(&request_bytes)
        .await
        .context("send request")?;
    send.finish().context("finish request")?;
    trace.step("send transfer request");

    let bytes_written = match ticket.kind() {
        PayloadKind::File | PayloadKind::Stdin => {
            if args.stdout {
                copy_to_stdout(recv, ticket.size(), show_progress).await?
            } else {
                let (path, plan) = file_target.expect("file target exists");
                let resume_from = match plan {
                    FilePlan::Download { resume_from } => resume_from,
                    FilePlan::Skip => 0,
                };
                let bytes = write_to_file(
                    recv,
                    path.clone(),
                    resume_from,
                    ticket.size(),
                    show_progress,
                )
                .await?;
                if let Some(algorithm) = args.checksum {
                    let value = crate::transport::source::checksum_path(path, algorithm).await?;
                    if args.json {
                        crate::json::emit(
                            "checksum",
                            &[
                                ("operation", crate::json::Value::String("recv")),
                                ("algorithm", crate::json::Value::String(algorithm.name())),
                                ("value", crate::json::Value::String(&value)),
                            ],
                        );
                    } else {
                        println!("checksum ({}): {}", algorithm.name(), value);
                    }
                }
                bytes
            }
        }
        PayloadKind::Dir => {
            if args.stdout {
                bail!("--stdout is not supported for directory tickets");
            }
            let (bytes, checksum) =
                extract_tar_stream(recv, out_dir, ticket.size(), show_progress, args.checksum)
                    .await?;
            if let (Some(algorithm), Some(value)) = (args.checksum, checksum) {
                if args.json {
                    crate::json::emit(
                        "checksum",
                        &[
                            ("operation", crate::json::Value::String("recv")),
                            ("algorithm", crate::json::Value::String(algorithm.name())),
                            ("value", crate::json::Value::String(&value)),
                        ],
                    );
                } else {
                    println!("checksum ({}): {}", algorithm.name(), value);
                }
            }
            bytes
        }
    };
    trace.step("receive payload");
    trace.info(format_args!("received {} bytes", bytes_written));

    conn.close(0u32.into(), b"done");
    endpoint.close().await;
    trace.finish(bytes_written);
    if json {
        crate::json::progress("recv", bytes_written);
        crate::json::emit(
            "completed",
            &[
                ("operation", crate::json::Value::String("recv")),
                ("bytes", crate::json::Value::Number(bytes_written)),
            ],
        );
    }
    Ok(())
}
async fn with_events_impl(
    args: RecvArgs,
    events: std::sync::mpsc::Sender<TransferEvent>,
) -> Result<()> {
    let _ = events.send(TransferEvent::Started);
    let result = run_impl(args).await;
    match &result {
        Ok(()) => {
            let _ = events.send(TransferEvent::Completed);
        }
        Err(err) => {
            let _ = events.send(TransferEvent::Failed(format!("{err:#}")));
        }
    }
    result
}
