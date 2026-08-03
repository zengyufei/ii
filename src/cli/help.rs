pub(crate) const HELP: &str = "\
ii file transfer

Usage:
  ii help [command]
  ii send [options] [path]
  ii web [directory] [--port <port>] [--token [value]] [--upload] [--path <dir>]
  ii webrtc [--port <port>] [--token [value]]
  ii tunnel (-s <target-host:port> | -c <ticket>) [--listen <ip:port>] [--relay <https-url> [-k]]
  ii recv [options] <ticket>
  ii relay [options]
  ii doctor
  ii version
";

pub(crate) const SEND_HELP: &str = "\
Usage:
  ii send [options] [path]

Options:
  --name <name>
  -t
  -c, --copy
  -o, --output <path>
  --s3
  --webdav
  --ftp
  --sftp
  --web
  --port <port>
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
  ii web [directory] [--port <port>] [--token [value]] [--upload] [--path <dir>]

Options:
  --port <port>
  --token [value]
  --upload
  --path <dir>
";

pub(crate) const WEBRTC_HELP: &str = "\
Usage:
  ii webrtc [--port <port>] [--token [value]]

Options:
  --port <port>
  --token [value]
";

pub(crate) const TUNNEL_HELP: &str = "\
Usage:
  ii tunnel -s <target-host:port> [--relay <https-url> [-k]]
  ii tunnel -c <ticket> [--listen <ip:port>]

Options:
  -s <target-host:port>  Serve a TCP target reachable from this computer
  -c <ticket>            Listen locally and connect to a tunnel ticket
  --listen <ip:port>     Local listener for -c; defaults to the first free 127.0.0.1 port from 8080
  --relay <https-url>    Force the serving endpoint through this relay
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
";

pub(crate) const RELAY_HELP: &str = "\
Usage:
  ii relay (--public <https-url> | --tls <domain> --cert <path> --key <path>) [-H <bind-port>]

Options:
  --public <https-url>  Self-signed mode; public HTTPS address including an optional port
  --tls <domain>        Manual TLS mode; certificate DNS name
  --cert <path>         PEM certificate chain for manual TLS mode
  --key <path>          PEM private key for manual TLS mode
  -H <bind-port>        Local HTTPS listener port; defaults to the public URL port or 443
";

pub(crate) const DOCTOR_HELP: &str = "Usage:\n  ii doctor";
pub(crate) const VERSION_HELP: &str = "Usage:\n  ii version";
