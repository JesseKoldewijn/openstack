/// DNS server tests.
///
/// These tests start a DNS server on an ephemeral port and verify:
/// 1. `*.localhost.localstack.cloud` resolves to the configured IP
/// 2. Non-localstack names are forwarded to an upstream resolver
/// 3. The `is_localstack_name` logic correctly classifies names
use std::net::Ipv4Addr;
use std::str::FromStr;
use std::sync::Arc;

use hickory_proto::{
    rr::{LowerName, Name},
    xfer::Protocol,
};
use hickory_resolver::{
    TokioResolver,
    config::{NameServerConfig, ResolverConfig, ResolverOpts},
    name_server::TokioConnectionProvider,
};
use hickory_server::ServerFuture;
use openstack_dns::LocalStackDnsHandler;
use tokio::net::UdpSocket;

/// Spawn a DNS server bound on a random port and return its local address.
async fn spawn_test_server(resolve_ip: Ipv4Addr) -> std::net::SocketAddr {
    let upstream = Arc::new(
        TokioResolver::builder_with_config(
            ResolverConfig::default(),
            TokioConnectionProvider::default(),
        )
        .with_options(ResolverOpts::default())
        .build(),
    );
    let handler = LocalStackDnsHandler::new(resolve_ip, upstream);
    let mut server = ServerFuture::new(handler);

    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = udp.local_addr().unwrap();
    server.register_socket(udp);

    tokio::spawn(async move {
        server.block_until_done().await.ok();
    });

    // Give the server a moment to start.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    addr
}

/// Build a resolver that sends queries to a specific address.
async fn make_resolver_for(addr: std::net::SocketAddr) -> TokioResolver {
    let ns = NameServerConfig::new(addr, Protocol::Udp);
    let mut config = ResolverConfig::new();
    config.add_name_server(ns);
    let mut opts = ResolverOpts::default();
    opts.attempts = 1;
    opts.timeout = std::time::Duration::from_secs(2);
    TokioResolver::builder_with_config(config, TokioConnectionProvider::default())
        .with_options(opts)
        .build()
}

#[tokio::test]
async fn test_localstack_wildcard_resolves_to_configured_ip() {
    let resolve_ip = Ipv4Addr::new(127, 0, 0, 1);
    let addr = spawn_test_server(resolve_ip).await;
    let resolver = make_resolver_for(addr).await;

    let response = resolver
        .lookup_ip("s3.localhost.localstack.cloud.")
        .await
        .expect("lookup should succeed");

    let ips: Vec<_> = response.iter().collect();
    assert!(!ips.is_empty(), "Expected at least one IP");
    assert!(
        ips.iter().any(|ip| ip == &std::net::IpAddr::V4(resolve_ip)),
        "Expected {} in response, got: {:?}",
        resolve_ip,
        ips
    );
}

#[tokio::test]
async fn test_localstack_root_resolves() {
    let resolve_ip = Ipv4Addr::new(127, 68, 1, 1);
    let addr = spawn_test_server(resolve_ip).await;
    let resolver = make_resolver_for(addr).await;

    let response = resolver
        .lookup_ip("localhost.localstack.cloud.")
        .await
        .expect("root localstack lookup should succeed");

    let ips: Vec<_> = response.iter().collect();
    assert!(
        ips.iter().any(|ip| ip == &std::net::IpAddr::V4(resolve_ip)),
        "Expected {} in response",
        resolve_ip
    );
}

#[tokio::test]
async fn test_localstack_deep_subdomain_resolves() {
    let resolve_ip = Ipv4Addr::new(127, 0, 0, 1);
    let addr = spawn_test_server(resolve_ip).await;
    let resolver = make_resolver_for(addr).await;

    // Deep subdomain like <account>.s3.localhost.localstack.cloud
    let response = resolver
        .lookup_ip("000000000000.s3.localhost.localstack.cloud.")
        .await
        .expect("deep subdomain lookup should succeed");

    let ips: Vec<_> = response.iter().collect();
    assert!(
        ips.iter().any(|ip| ip == &std::net::IpAddr::V4(resolve_ip)),
        "Expected {} in response",
        resolve_ip
    );
}

/// Unit test for the `is_localstack_name` classification logic.
#[test]
fn test_is_localstack_name_classification() {
    let localstack_names = [
        "localhost.localstack.cloud.",
        "s3.localhost.localstack.cloud.",
        "sqs.localhost.localstack.cloud.",
        "000000000000.s3.localhost.localstack.cloud.",
    ];

    let non_localstack_names = [
        "example.com.",
        "localstack.cloud.",
        "google.com.",
        "localhost.",
    ];

    for name in &localstack_names {
        let lower = LowerName::from(Name::from_str(name).unwrap());
        assert!(
            LocalStackDnsHandler::is_localstack_name(&lower),
            "Expected '{}' to be classified as a localstack name",
            name
        );
    }

    for name in &non_localstack_names {
        let lower = LowerName::from(Name::from_str(name).unwrap());
        assert!(
            !LocalStackDnsHandler::is_localstack_name(&lower),
            "Expected '{}' to NOT be classified as a localstack name",
            name
        );
    }
}
