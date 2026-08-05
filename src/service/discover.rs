use crate::{
    command::DiscoverArgs,
    discovery::{self, Service},
};
use anyhow::Result;

pub(super) async fn run(args: DiscoverArgs) -> Result<()> {
    if args.json {
        crate::json::started("discover");
    }
    let services = discovery::discover().await?;
    for service in &services {
        if args.json {
            match service {
                Service::Send {
                    ticket,
                    name,
                    kind,
                    size,
                } => crate::json::emit(
                    "service",
                    &[
                        ("service", crate::json::Value::String("send")),
                        ("ticket", crate::json::Value::String(ticket)),
                        ("name", crate::json::Value::String(name)),
                        ("kind", crate::json::Value::String(kind)),
                        (
                            "size",
                            size.map(crate::json::Value::Number)
                                .unwrap_or(crate::json::Value::Null),
                        ),
                    ],
                ),
                Service::Web { url } => crate::json::emit(
                    "service",
                    &[
                        ("service", crate::json::Value::String("web")),
                        ("url", crate::json::Value::String(url)),
                    ],
                ),
                Service::Dav { url } => crate::json::emit(
                    "service",
                    &[
                        ("service", crate::json::Value::String("dav")),
                        ("url", crate::json::Value::String(url)),
                    ],
                ),
            }
        } else {
            match service {
                Service::Send { ticket, name, .. } => {
                    println!("send: {name}");
                    println!("ii recv {ticket}");
                }
                Service::Web { url } => println!("web: {url}"),
                Service::Dav { url } => println!("dav: {url}"),
            }
        }
    }
    if args.json {
        crate::json::emit(
            "completed",
            &[
                ("operation", crate::json::Value::String("discover")),
                ("count", crate::json::Value::Number(services.len() as u64)),
            ],
        );
    }
    Ok(())
}
