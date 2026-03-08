use std::{net::Ipv4Addr, sync::Arc, time::Duration};

use hickory_proto::{
    op::{Header, ResponseCode},
    rr::{LowerName, Name, RData, Record, RecordType, rdata::A},
};
use hickory_resolver::TokioResolver;
use hickory_server::{
    authority::MessageResponseBuilder,
    server::{Request, RequestHandler, ResponseHandler, ResponseInfo},
};
use openstack_config::Config;
use tracing::{debug, info, warn};

/// Suffix we intercept.
const LOCALSTACK_SUFFIX: &str = ".localhost.localstack.cloud";

/// Build a ServFail `ResponseInfo` header.
fn serve_failed_info() -> ResponseInfo {
    let mut header = Header::new();
    header.set_response_code(ResponseCode::ServFail);
    header.into()
}

/// Handler that resolves `*.localhost.localstack.cloud` → `DNS_RESOLVE_IP`
/// and forwards everything else to an upstream resolver.
pub struct LocalStackDnsHandler {
    resolve_ip: Ipv4Addr,
    upstream: Arc<TokioResolver>,
}

impl LocalStackDnsHandler {
    pub fn new(resolve_ip: Ipv4Addr, upstream: Arc<TokioResolver>) -> Self {
        Self {
            resolve_ip,
            upstream,
        }
    }

    /// Returns true if the name matches `*.localhost.localstack.cloud` or
    /// exactly `localhost.localstack.cloud`.
    pub fn is_localstack_name(name: &LowerName) -> bool {
        let name_str = name.to_string();
        // LowerName::to_string() includes a trailing dot.
        let name_str = name_str.trim_end_matches('.');
        name_str == "localhost.localstack.cloud" || name_str.ends_with(LOCALSTACK_SUFFIX)
    }
}

#[async_trait::async_trait]
impl RequestHandler for LocalStackDnsHandler {
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        mut response_handle: R,
    ) -> ResponseInfo {
        let info = match request.request_info() {
            Ok(info) => info,
            Err(e) => {
                warn!("Malformed DNS request: {}", e);
                return serve_failed_info();
            }
        };
        let query = info.query;
        let lower_name: &LowerName = query.name();
        let query_type = query.query_type();

        debug!("DNS query: {} {:?}", lower_name, query_type);

        let builder = MessageResponseBuilder::from_message_request(request);
        let mut header = Header::response_from_request(request.header());

        // Only handle A queries for localstack names here; forward everything else.
        if Self::is_localstack_name(lower_name) && query_type == RecordType::A {
            let name_owned: Name = lower_name.clone().into();
            info!("Resolving {} -> {}", name_owned, self.resolve_ip);
            header.set_response_code(ResponseCode::NoError);
            header.set_authoritative(true);

            let record = Record::from_rdata(name_owned, 300, RData::A(A(self.resolve_ip)));
            let records = [record];

            let response = builder.build(
                header,
                records.iter(),
                std::iter::empty::<&Record>(),
                std::iter::empty::<&Record>(),
                std::iter::empty::<&Record>(),
            );
            match response_handle.send_response(response).await {
                Ok(info) => info,
                Err(e) => {
                    warn!("Failed to send DNS response: {}", e);
                    serve_failed_info()
                }
            }
        } else {
            // Forward to upstream resolver.
            let name_owned: Name = lower_name.clone().into();
            match self.upstream.lookup(name_owned.clone(), query_type).await {
                Ok(lookup) => {
                    header.set_response_code(ResponseCode::NoError);
                    let records: Vec<Record> = lookup.record_iter().cloned().collect::<Vec<_>>();
                    let response = builder.build(
                        header,
                        records.iter(),
                        std::iter::empty::<&Record>(),
                        std::iter::empty::<&Record>(),
                        std::iter::empty::<&Record>(),
                    );
                    match response_handle.send_response(response).await {
                        Ok(info) => info,
                        Err(e) => {
                            warn!("Failed to send upstream DNS response: {}", e);
                            serve_failed_info()
                        }
                    }
                }
                Err(e) => {
                    debug!("Upstream DNS lookup failed for {}: {}", name_owned, e);
                    header.set_response_code(ResponseCode::NXDomain);
                    let response = builder.build_no_records(header);
                    match response_handle.send_response(response).await {
                        Ok(info) => info,
                        Err(send_err) => {
                            warn!("Failed to send NXDOMAIN response: {}", send_err);
                            serve_failed_info()
                        }
                    }
                }
            }
        }
    }
}

/// Embedded DNS server that resolves *.localhost.localstack.cloud to DNS_RESOLVE_IP.
pub struct DnsServer {
    config: Config,
}

impl DnsServer {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub async fn run(&self) -> Result<(), anyhow::Error> {
        let addr = self.config.dns_address.as_deref().unwrap_or("0.0.0.0");
        let port = self.config.dns_port;
        let resolve_ip: Ipv4Addr = self
            .config
            .dns_resolve_ip
            .parse()
            .unwrap_or(Ipv4Addr::new(127, 0, 0, 1));

        info!("Starting DNS server on {}:{}", addr, port);
        info!("Resolving *.localhost.localstack.cloud to {}", resolve_ip);

        // Build upstream resolver using system defaults.
        let upstream = TokioResolver::builder_tokio()?.build();
        let upstream = Arc::new(upstream);

        let handler = LocalStackDnsHandler::new(resolve_ip, upstream);
        let mut server = hickory_server::ServerFuture::new(handler);

        let bind_addr = format!("{}:{}", addr, port);

        // Register UDP socket.
        let udp_socket = tokio::net::UdpSocket::bind(&bind_addr).await?;
        server.register_socket(udp_socket);

        // Register TCP listener.
        let tcp_listener = tokio::net::TcpListener::bind(&bind_addr).await?;
        server.register_listener(tcp_listener, Duration::from_secs(5));

        info!("DNS server listening on {}", bind_addr);
        server.block_until_done().await?;

        Ok(())
    }
}
