/// Performance tests for OpenSearch provider.
///
/// These cover domain creation, listing, and config update paths.
use std::collections::HashMap;
use std::time::Instant;

use bytes::Bytes;
use openstack_opensearch::OpenSearchProvider;
use openstack_service_framework::traits::{DispatchResponse, RequestContext, ServiceProvider};
use serde_json::{Value, json};

fn make_ctx(operation: &str, body: Value, path: &str, method: &str) -> RequestContext {
    RequestContext {
        service: "opensearch".to_string(),
        operation: operation.to_string(),
        region: "us-east-1".to_string(),
        account_id: "000000000000".to_string(),
        request_body: body.clone(),
        raw_body: Some(Bytes::from(serde_json::to_vec(&body).unwrap())),
        headers: Default::default(),
        path: path.to_string(),
        method: method.to_string(),
        query_params: HashMap::new(),
        request_id: String::new(),
        spooled_body: None,
        body_reader: None,
    }
}

fn body(resp: &DispatchResponse) -> Value {
    serde_json::from_slice(resp.body.as_bytes()).expect("valid JSON")
}

async fn create_domain(p: &OpenSearchProvider, name: &str) {
    let resp = p
        .dispatch(&make_ctx(
            "CreateDomain",
            json!({ "DomainName": name }),
            "/2021-01-01/opensearch/domain",
            "POST",
        ))
        .await
        .unwrap();
    assert_eq!(resp.status_code, 200);
}

#[tokio::test]
async fn perf_create_domain_throughput() {
    let p = OpenSearchProvider::new();
    let n = 100usize;

    let start = Instant::now();
    for i in 0..n {
        create_domain(&p, &format!("perf-domain-{i:03}")).await;
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 2000,
        "CreateDomain x{n} took {}ms — expected <2000ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_list_domain_names_many() {
    let p = OpenSearchProvider::new();
    let n = 100usize;
    for i in 0..n {
        create_domain(&p, &format!("perf-list-domain-{i:03}")).await;
    }

    let start = Instant::now();
    let resp = p
        .dispatch(&make_ctx(
            "ListDomainNames",
            json!({}),
            "/2021-01-01/opensearch/domain",
            "GET",
        ))
        .await
        .unwrap();
    let elapsed = start.elapsed();
    assert_eq!(resp.status_code, 200);
    let b = body(&resp);
    assert!(b["DomainNames"].as_array().unwrap().len() >= n);

    assert!(
        elapsed.as_millis() < 500,
        "ListDomainNames({n}) took {}ms — expected <500ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn perf_update_domain_config_round_trip() {
    let p = OpenSearchProvider::new();
    create_domain(&p, "perf-update-domain").await;

    let start = Instant::now();
    for i in 0..100usize {
        let resp = p
            .dispatch(&make_ctx(
                "UpdateDomainConfig",
                json!({
                    "EngineVersion": "OpenSearch_2.11",
                    "ClusterConfig": {
                        "InstanceType": "m6g.large.search",
                        "InstanceCount": (i % 3) + 1
                    }
                }),
                "/2021-01-01/opensearch/domain/perf-update-domain/config",
                "POST",
            ))
            .await
            .unwrap();
        assert_eq!(resp.status_code, 200);
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 1000,
        "UpdateDomainConfig x100 took {}ms — expected <1000ms",
        elapsed.as_millis()
    );
}
