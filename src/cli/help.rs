pub(crate) const HELP: &str = "\
ii file transfer

Usage:
  ii help [command]
  ii send [options] [path...]
  ii watch <directory> [options]
  ii queue <path...> [--after <duration>|--every <duration>] [options]
  ii web [directory] [--port <port>] [--bind <ip>] [--token [value]] [--upload] [--path <dir>] [--once]
  ii dav [directory] [--port <port>] [--bind <ip>] [--token [value]] [--read-only] [--username <username> --password <password>] [--tls [--domain <name>] [--cert <path> --key <path>]]
  ii socks5 [--port <port>] [--bind <ip>] [--username <user> --password <pass>]
  ii http [directory] [--port <port>] [--bind <ip>] [--token [value]]
  ii paste [text] [--port <port>] [--bind <ip>] [--token [value]] [--ttl <duration>]
  ii drop [directory] [--port <port>] [--bind <ip>] [--token [value]]
  ii ftp [directory] [--bind <ip>] [--port <port>] [--username <username> --password <password>] [--rate <bytes/s>] [--max <n>] [--upload <true|false>] [--download <true|false>] [--delete <true|false>] [--rename <true|false>] [--mkdir <true|false>] [--tls [--implicit] [--cert <path> --key <path>]] [--passive-host <IPv4|hostname>] [--passive-ports [start-end]]
  ii proxy [--port <port>] [--bind <ip>] [--username <user> --password <pass>]
  ii tcp <host:port> [--port <port>] [--bind <ip>]
  ii udp <host:port> [--port <port>] [--bind <ip>]
  ii ping <host:port> [--count <n>] [--interval <duration>] [--timeout <duration>]
  ii speed (serve [--port <port>] [--bind <ip>] [--token [value]] | <http-url> [--duration <duration>])
  ii wake <mac> [--broadcast <ip>] [--port <port>]
  ii port <host> <port...> [--timeout <duration>]
  ii health <http-url|host:port> [--interval <duration>] [--timeout <duration>]
  ii pac --proxy <http://host:port|socks5://host:port> [--port <port>] [--bind <ip>] [--token [value]]
  ii webrtc [--port <port>] [--bind <ip>] [--token [value]]
  ii tunnel (-s <target-host:port> | -c <ticket>) [--listen <ip:port>] [--relay <url> [-k]]
  ii recv [options] <ticket>
  ii discover [--json]
  ii relay [options]
  ii doctor [--nat]
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
  --checksum <md5|sha256>
  --preserve-metadata
  --symlinks <follow|preserve|reject>
  --quic-port <port>
  -t                    Keep serving after success; required for repeated --web downloads
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
  ii web [directory] [--port <port>] [--bind <ip>] [--token [value]] [--upload] [--path <dir>] [--once]

Options:
  --port <port>
  --bind <ip>
  --token [value]
  --upload
  --path <dir>
  --once                Stop after the first complete file download
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
  --checksum <md5|sha256>
  --quic-port <port>
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

pub(crate) const WATCH_HELP: &str = "\
Usage:
  ii watch <directory> [options]

Options:
  --interval <duration>   Scan interval; defaults to 2s
  --stabilize <duration>  Stable time before sending; defaults to 2s
  --checksum <md5|sha256>
  --preserve-metadata
  --symlinks <follow|preserve|reject>
  --quic-port <port>
  --rate <bytes/s>
  --include <glob>
  --exclude <glob>
  --s3, --r2, --azure, --webdav, --ftp, --sftp
  --local, --relay <url>, --no-relay
";

pub(crate) const QUEUE_HELP: &str = "\
Usage:
  ii queue <path...> [--after <duration>|--every <duration>] [options]

Options:
  --after <duration>       Run once after this delay
  --every <duration>       Repeat after each FIFO round
  --checksum <md5|sha256>
  --preserve-metadata
  --symlinks <follow|preserve|reject>
  --quic-port <port>
  --rate <bytes/s>
  --include <glob>
  --exclude <glob>
  --s3, --r2, --azure, --webdav, --ftp, --sftp
  --local, --relay <url>, --no-relay
";

pub(crate) const DAV_HELP: &str = "\
Usage:
  ii dav [directory] [--port <port>] [--bind <ip>] [--token [value]] [--read-only] [--username <username> --password <password>] [--tls [--domain <name>] [--cert <path> --key <path>]]

Options:
  --port <port>
  --bind <ip>
  --token [value]
  --read-only
  --username <username>  HTTP Basic Auth username; requires --password
  --password <password>  HTTP Basic Auth password; requires --username
  --tls                  Enable HTTPS with an ii-generated self-signed certificate
  --domain <name>        TLS DNS name used for the advertised URL and self-signed certificate
  --cert <path>          PEM certificate chain; replaces the generated certificate
  --key <path>           PEM private key; requires --cert
";

pub(crate) const SOCKS5_HELP: &str = "\
Usage:
  ii socks5 [--port <port>] [--bind <ip>] [--username <user> --password <pass>]

Options:
  --port <port>              Listener port; defaults to a random free port
  --bind <ip>                Listener IPv4 or IPv6 address; defaults to 0.0.0.0
  --username <user>          Enable SOCKS5 username/password authentication
  --password <pass>          SOCKS5 password; requires --username
";

pub(crate) const HTTP_HELP: &str = "\
Usage:
  ii http [directory] [--port <port>] [--bind <ip>] [--token [value]]

Options:
  --port <port>
  --bind <ip>
  --token [value]
";

pub(crate) const PASTE_HELP: &str = "\
Usage:
  ii paste [text] [--port <port>] [--bind <ip>] [--token [value]] [--ttl <duration>]

Without text, reads stdin until EOF. Use -- before text beginning with '-'.

Options:
  --port <port>
  --bind <ip>
  --token [value]
  --ttl <duration>
";

pub(crate) const DROP_HELP: &str = "\
Usage:
  ii drop [directory] [--port <port>] [--bind <ip>] [--token [value]]

Options:
  --port <port>
  --bind <ip>
  --token [value]
";

pub(crate) const FTP_HELP: &str = "\
Usage:
  ii ftp [directory] [--bind <ip>] [--port <port>] [--username <username> --password <password>] [--rate <bytes/s>] [--max <n>] [--upload <true|false>] [--download <true|false>] [--delete <true|false>] [--rename <true|false>] [--mkdir <true|false>] [--tls [--implicit] [--cert <path> --key <path>]] [--passive-host <IPv4|hostname>] [--passive-ports [start-end]]

Options:
  --bind <ip>                 Control listener address; defaults to 0.0.0.0
  --port <port>               Control listener port; defaults to 21
  --username <username>       Login username; requires --password
  --password <password>       Login password; requires --username
  --rate <bytes/s>            Shared upload/download rate limit
  --max <n>                   Maximum control connections; defaults to 100
  --upload <true|false>       Allow file uploads; defaults to true
  --download <true|false>     Allow file downloads; defaults to true
  --delete <true|false>       Allow file and directory deletion; defaults to true
  --rename <true|false>       Allow renaming; defaults to true
  --mkdir <true|false>        Allow directory creation; defaults to true
  --tls                        Require FTPS; generates a temporary self-signed certificate unless --cert and --key are supplied
  --implicit                   Use implicit FTPS; requires --tls and defaults to port 990
  --cert <path>                PEM certificate chain; requires --tls and --key
  --key <path>                 PEM private key; requires --tls and --cert
  --passive-host <value>      PASV advertised IPv4 or DNS name; requires --passive-ports
  --passive-ports [a-b]       Enable passive mode alongside active mode; defaults to 49152-65535
";

pub(crate) const PROXY_HELP: &str = "\
Usage:
  ii proxy [--port <port>] [--bind <ip>] [--username <user> --password <pass>]

Options:
  --port <port>
  --bind <ip>
  --username <user>
  --password <pass>
";

pub(crate) const TCP_HELP: &str = "\
Usage:
  ii tcp <host:port> [--port <port>] [--bind <ip>]
";

pub(crate) const UDP_HELP: &str = "\
Usage:
  ii udp <host:port> [--port <port>] [--bind <ip>]
";

pub(crate) const PING_HELP: &str = "\
Usage:
  ii ping <host:port> [--count <n>] [--interval <duration>] [--timeout <duration>]
";

pub(crate) const SPEED_HELP: &str = "\
Usage:
  ii speed serve [--port <port>] [--bind <ip>] [--token [value]]
  ii speed <http-url> [--duration <duration>]
";

pub(crate) const WAKE_HELP: &str = "\
Usage:
  ii wake <mac> [--broadcast <ip>] [--port <port>]
";

pub(crate) const PORT_HELP: &str = "\
Usage:
  ii port <host> <port...> [--timeout <duration>]
";

pub(crate) const HEALTH_HELP: &str = "\
Usage:
  ii health <http-url|host:port> [--interval <duration>] [--timeout <duration>]
";

pub(crate) const PAC_HELP: &str = "\
Usage:
  ii pac --proxy <http://host:port|socks5://host:port> [--port <port>] [--bind <ip>] [--token [value]]
";

pub(crate) const DISCOVER_HELP: &str = "\
Usage:
  ii discover [--json]

Options:
  --json
";

pub(crate) const DOCTOR_HELP: &str = "\
Usage:
  ii doctor [--nat]

Options:
  --nat                 Run a short-lived UDP/NAT and relay reachability probe
";
pub(crate) const VERSION_HELP: &str = "Usage:\n  ii version";
