use clap::{self, Parser, Subcommand};
use log::*;
use serde::{self, Deserialize};
use tokio::{
    io::{self, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf},
    net::TcpListener,
    runtime::Builder,
    time::Duration,
};

#[tokio::main]
async fn main() {
    let proxy_configuration = ProxyConfiguration {
        bind_address: "127.0.0.1:10086",
    };
    let mut tcp_listener = TcpListener::bind(&proxy_configuration.bind_address)
        .await
        .map_err(|e| {
            error!(
                "Error binding address {}: {}",
                &proxy_configuration.bind_address, e
            );
            e
        })?;

    let dns_resolver = DnsResolver {};
    let config = Config {};
    loop {
        // Asynchronously wait for an inbound socket.
        let socket = tcp_listener.accept().await;

        let dns_resolver_ref = dns_resolver.clone();

        match socket {
            Ok((stream, _)) => {
                let config = config.clone();
                // handle accepted connections asynchronously
                tokio::spawn(async move { tunnel_stream(&config, stream, dns_resolver_ref).await });
            }
            Err(e) => error!("Failed TCP handshake {}", e),
        }
    }
}

pub struct ProxyConfiguration {
    bind_address: str,
}

pub struct DnsResolver {}

pub struct Config {}

/// Tunnel via a client connection.
async fn tunnel_stream<C: AsyncRead + AsyncWrite + Send + Unpin + 'static>(
    config: &ProxyConfiguration,
    client_connection: C,
    dns_resolver: DnsResolver,
) -> io::Result<()> {
    let (client_recv, client_send) = io::split(client);
    let (target_recv, target_send) = io::split(target);

    let upstream_task =
        tokio::spawn(async move { upstream_relay.relay_data(client_recv, target_send).await });

    let downstream_task =
        tokio::spawn(async move { downstream_relay.relay_data(target_recv, client_send).await });

    let read_result = self
        .relay_policy
        .timed_operation(source.read(&mut buffer))
        .await;

    if read_result.is_err() {
        shutdown_reason = RelayShutdownReasons::ReaderTimeout;
        break;
    }

    let n = match read_result.unwrap() {
        Ok(n) if n == 0 => {
            shutdown_reason = RelayShutdownReasons::GracefulShutdown;
            break;
        }
        Ok(n) => n,
        Err(e) => {
            error!(
                "{} failed to read. Err = {:?}, CTX={}",
                self.name, e, self.tunnel_ctx
            );
            shutdown_reason = RelayShutdownReasons::ReadError;
            break;
        }
    };
}

#[async_trait]
pub trait TunnelTarget {
    type Addr;
    fn addr(&self) -> Self::Addr;
}

#[async_trait]
pub trait TargetConnector {
    type Target: TunnelTarget + Send + Sync + Sized;
    type Stream: AsyncRead + AsyncWrite + Send + Sized + 'static;

    async fn connect(&mut self, target: &Self::Target) -> io::Result<Self::Stream>;
}

/// Relays data in a single direction. E.g.
pub async fn relay_data<R: AsyncReadExt + Sized, W: AsyncWriteExt + Sized>(
    self,
    mut source: ReadHalf<R>,
    mut dest: WriteHalf<W>,
) -> io::Result<RelayStats> {
}

/// A connection tunnel.
///
/// # Parameters
/// * `<H>` - proxy handshake codec for initiating a tunnel.
///    It extracts the request message, which contains the target, and, potentially policies.
///    It also takes care of encoding a response.
/// * `<C>` - a connection from from client.
/// * `<T>` - target connector. It takes result produced by the codec and establishes a connection
///           to a target.
///
/// Once the target connection is established, it relays data until any connection is closed or an
/// error happens.
impl<H, C, T> ConnectionTunnel<H, C, T>
where
    H: Decoder<Error = EstablishTunnelResult> + Encoder<EstablishTunnelResult>,
    H::Item: TunnelTarget + Sized + Display + Send + Sync,
    C: AsyncRead + AsyncWrite + Sized + Send + Unpin + 'static,
    T: TargetConnector<Target = H::Item>,
{
}

pub enum EstablishTunnelResult {
    /// Successfully connected to target.  
    Ok,
    /// Malformed request
    BadRequest,
    /// Target is not allowed
    Forbidden,
    /// Unsupported operation, however valid for the protocol.
    OperationNotAllowed,
    /// The client failed to send a tunnel request timely.
    RequestTimeout,
    /// Cannot connect to target.
    BadGateway,
    /// Connection attempt timed out.
    GatewayTimeout,
    /// Busy. Try again later.
    TooManyRequests,
    /// Any other error. E.g. an abrupt I/O error.
    ServerError,
}

/// Stats after the relay is closed. Can be used for telemetry/monitoring.
#[derive(Builder, Clone, Debug, Serialize)]
pub struct RelayStats {
    pub shutdown_reason: RelayShutdownReasons,
    pub total_bytes: usize,
    pub event_count: usize,
    pub duration: Duration,
}

/// Statistics. No sensitive information.
#[derive(Serialize)]
pub struct TunnelStats {
    tunnel_ctx: TunnelCtx,
    result: EstablishTunnelResult,
    upstream_stats: Option<RelayStats>,
    downstream_stats: Option<RelayStats>,
}

#[tokio::test]
async fn test_timed_operation_timeout() {
    let time_duration = 1;
    let data = b"data on the wire";
    let mut mock_connection: Mock = Builder::new()
        .wait(Duration::from_secs(time_duration * 2))
        .read(data)
        .build();

    let relay_policy: RelayPolicy = RelayPolicyBuilder::default()
        .min_rate_bpm(1000)
        .max_rate_bps(100_000)
        .idle_timeout(Duration::from_secs(time_duration))
        .build()
        .unwrap();

    let mut buf = [0; 1024];
    let timed_future = relay_policy
        .timed_operation(mock_connection.read(&mut buf))
        .await;
    assert!(timed_future.is_err());
}

#[tokio::test]
async fn test_timed_operation_failed_io() {
    let mut mock_connection: Mock = Builder::new()
        .read_error(Error::from(ErrorKind::BrokenPipe))
        .build();

    let relay_policy: RelayPolicy = RelayPolicyBuilder::default()
        .min_rate_bpm(1000)
        .max_rate_bps(100_000)
        .idle_timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let mut buf = [0; 1024];
    let timed_future = relay_policy
        .timed_operation(mock_connection.read(&mut buf))
        .await;
    assert!(timed_future.is_ok()); // no timeout
    assert!(timed_future.unwrap().is_err()); // but io-error
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
struct Cli {
    /// Configuration file.
    #[clap(long)]
    config: Option<String>,
    /// Bind address, e.g. 0.0.0.0:8443.
    #[clap(long)]
    bind: String,
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Http(HttpOptions),
    Https(HttpsOptions),
    Tcp(TcpOptions),
}

#[derive(Deserialize, Clone)]
pub struct TargetConnectionConfig {
    #[serde(with = "humantime_serde")]
    pub dns_cache_ttl: Duration,
    #[serde(with = "serde_regex")]
    pub allowed_targets: Regex,
    #[serde(with = "humantime_serde")]
    pub connect_timeout: Duration,
    pub relay_policy: RelayPolicy,
}

#[derive(Builder, Deserialize, Clone)]
pub struct RelayPolicy {
    #[serde(with = "humantime_serde")]
    pub idle_timeout: Duration,
    /// Min bytes-per-minute (bpm)
    pub min_rate_bpm: u64,
    // Max bytes-per-second (bps)
    pub max_rate_bps: u64,
}
