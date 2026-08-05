pub(crate) const HELP: &str = "\
ii file transfer

Usage:
  ii help [command]
  ii send [options] [path...]
  ii web [directory] [--port <port>] [--bind <ip>] [--token [value]] [--upload] [--path <dir>]
  ii dav [directory] [--port <port>] [--bind <ip>] [--token [value]] [--read-only]
  ii webrtc [--port <port>] [--bind <ip>] [--token [value]]
  ii tunnel (-s <target-host:port> | -c <ticket>) [--listen <ip:port>] [--relay <url> [-k]]
  ii recv [options] <ticket>
  ii discover [--json]
  ii relay [options]
  ii doctor
  ii version
";

pub(crate) const SEND_HELP: &str = "\
Usage:
  ii send [options] [path...]

Options:
  --name <name>
  --include <glob>
  --exclude <glob>
  --rate <bytes/s>
  --json
  -t
  -c, --copy
  -o, --output <path>
  --s3                 Generic S3-compatible object storage
  --r2                 Cloudflare R2
  --azure              Azure Blob Storage (Shared Key or SAS)
  --webdav
  --ftp
  --sftp
  --web
  --port <port>
  --bind <ip>
  --token [value]
  --upload
  --path <dir>
  -p
  -d
  --profile <name>
  --local
  --relay <url>
  -k
  --no-relay
";

pub(crate) const WEB_HELP: &str = "\
Usage:
  ii web [directory] [--port <port>] [--bind <ip>] [--token [value]] [--upload] [--path <dir>]

Options:
  --port <port>
  --bind <ip>
  --token [value]
  --upload
  --path <dir>
";

pub(crate) const WEBRTC_HELP: &str = "\
Usage:
  ii webrtc [--port <port>] [--bind <ip>] [--token [value]]

Options:
  --port <port>
  --bind <ip>
  --token [value]
";

pub(crate) const TUNNEL_HELP: &str = "\
Usage:
  ii tunnel -s <target-host:port> [--relay <url> [-k]]
  ii tunnel -c <ticket> [--listen <ip:port>]

Options:
  -s <target-host:port>  Serve a TCP target reachable from this computer
  -c <ticket>            Listen locally and connect to a tunnel ticket
  --listen <ip:port>     Local listener for -c; defaults to the first free 127.0.0.1 port from 8080
  --relay <url>           Force the serving endpoint through this relay
  -k                     Accept a self-signed --relay certificate
";

pub(crate) const RECV_HELP: &str = "\
Usage:
  ii recv [options] <ticket>

Options:
  -o <dir>
  --stdout
  --overwrite
  --resume
  --local
  --trace
  --json
";

pub(crate) const RELAY_HELP: &str = "\
Usage:
  ii relay [--port <port>] [--tls [--domain <name>] [--cert <path> --key <path>]]

Options:
  --port <port>         HTTP or HTTPS listener port; defaults to a random free port
  --bind <ip>           Listener IPv4 or IPv6 address; defaults to 0.0.0.0
  --tls                 Enable HTTPS with an ii-generated self-signed certificate
  --domain <name>       TLS DNS name used for the advertised URL and self-signed certificate
  --cert <path>         PEM certificate chain; replaces the generated certificate
  --key <path>          PEM private key; requires --cert
";

pub(crate) const DAV_HELP: &str = "\
Usage:
  ii dav [directory] [--port <port>] [--bind <ip>] [--token [value]] [--read-only]

Options:
  --port <port>
  --bind <ip>
  --token [value]
  --read-only
";

pub(crate) const DISCOVER_HELP: &str = "\
Usage:
  ii discover [--json]

Options:
  --json
";

pub(crate) const DOCTOR_HELP: &str = "Usage:\n  ii doctor";
pub(crate) const VERSION_HELP: &str = "Usage:\n  ii version";
