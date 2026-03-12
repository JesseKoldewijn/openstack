use anyhow::Result;
use tracing::{debug, info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    if handle_cli_shortcuts() {
        return Ok(());
    }

    // Initialize configuration from environment
    let config = openstack_config::Config::from_env()?;

    // Initialize tracing/logging
    openstack_config::logging::init(&config);

    info!("Starting openstack v{}", env!("CARGO_PKG_VERSION"));
    info!("Gateway listening on: {:?}", config.gateway_listen);

    // Initialize the service plugin manager
    let plugin_manager = openstack_service_framework::ServicePluginManager::new(config.clone());

    // Register all built-in service providers and collect persistable stores
    let persistable_stores = register_services(&plugin_manager, &config);

    // Initialize state and register persistable stores before loading
    let state_manager = openstack_state::StateManager::new(config.clone());
    for store in persistable_stores {
        state_manager.register_store(store).await;
    }
    state_manager.load_on_startup().await?;

    // Clean up orphaned spool temp files from previous crashes
    cleanup_spool_dir(&config.directories.spool).await;

    // Start internal API server and gateway
    let gateway = openstack_gateway::Gateway::new(config.clone(), plugin_manager.clone());

    // Start DNS server if configured
    let dns_handle = if config.dns_enabled() {
        let dns = openstack_dns::DnsServer::new(config.clone());
        Some(tokio::spawn(async move { dns.run().await }))
    } else {
        None
    };

    // Run the gateway (blocks until shutdown)
    gateway.run(state_manager).await?;

    // Shutdown DNS if running
    if let Some(handle) = dns_handle {
        handle.abort();
    }

    info!("openstack shut down cleanly");
    Ok(())
}

fn handle_cli_shortcuts() -> bool {
    let mut args = std::env::args().skip(1);
    let Some(first) = args.next() else {
        return false;
    };

    if args.next().is_some() {
        return false;
    }

    match first.as_str() {
        "-h" | "--help" => {
            println!("openstack {}", env!("CARGO_PKG_VERSION"));
            println!("Usage: openstack [--help] [--version]");
            println!(
                "Starts the openstack local AWS emulator with environment-based configuration."
            );
            true
        }
        "-V" | "--version" => {
            println!("openstack {}", env!("CARGO_PKG_VERSION"));
            true
        }
        _ => false,
    }
}

fn register_services(
    manager: &openstack_service_framework::ServicePluginManager,
    config: &openstack_config::Config,
) -> Vec<std::sync::Arc<dyn openstack_state::PersistableStore>> {
    let services = &config.services;
    let mut persistable_stores: Vec<std::sync::Arc<dyn openstack_state::PersistableStore>> =
        Vec::new();

    // S3 is special: it has a persistable store that shares state with the provider
    if services.is_enabled("s3") {
        let s3_provider = openstack_s3::S3Provider::new(&config.directories.s3_objects);
        persistable_stores.push(s3_provider.persistable_store());
        manager.register("s3", s3_provider);
    }

    macro_rules! register {
        ($name:literal, $provider:expr) => {
            if services.is_enabled($name) {
                manager.register($name, $provider);
            }
        };
    }

    register!("sqs", openstack_sqs::SqsProvider::new());
    register!("sns", openstack_sns::SnsProvider::new());
    register!("dynamodb", openstack_dynamodb::DynamoDbProvider::new());
    register!("lambda", openstack_lambda::LambdaProvider::new());
    register!("iam", openstack_iam::IamProvider::new());
    register!("sts", openstack_sts::StsProvider::new());
    register!("kms", openstack_kms::KmsProvider::new());
    register!(
        "cloudformation",
        openstack_cloudformation::CloudFormationProvider::new()
    );
    register!(
        "cloudwatch",
        openstack_cloudwatch::CloudWatchProvider::new()
    );
    register!("kinesis", openstack_kinesis::KinesisProvider::new());
    register!("firehose", openstack_firehose::FirehoseProvider::new());
    register!("events", openstack_eventbridge::EventBridgeProvider::new());
    register!(
        "states",
        openstack_stepfunctions::StepFunctionsProvider::new()
    );
    register!(
        "apigateway",
        openstack_apigateway::ApiGatewayProvider::new()
    );
    register!("ec2", openstack_ec2::Ec2Provider::new());
    register!("route53", openstack_route53::Route53Provider::new());
    register!("ses", openstack_ses::SesProvider::new());
    register!("ssm", openstack_ssm::SsmProvider::new());
    register!(
        "secretsmanager",
        openstack_secretsmanager::SecretsManagerProvider::new()
    );
    register!("acm", openstack_acm::AcmProvider::new());
    register!("ecr", openstack_ecr::EcrProvider::new());
    register!(
        "opensearch",
        openstack_opensearch::OpenSearchProvider::new()
    );
    register!("redshift", openstack_redshift::RedshiftProvider::new());

    persistable_stores
}

/// Remove any `.tmp` files left in the spool directory from a previous crash.
async fn cleanup_spool_dir(spool_dir: &std::path::Path) {
    let Ok(mut rd) = tokio::fs::read_dir(spool_dir).await else {
        // Directory may not exist yet — that's fine.
        return;
    };
    let mut cleaned = 0u64;
    while let Ok(Some(entry)) = rd.next_entry().await {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "tmp") {
            if let Err(e) = tokio::fs::remove_file(&path).await {
                warn!("Failed to remove orphaned spool file {:?}: {}", path, e);
            } else {
                cleaned += 1;
            }
        }
    }
    if cleaned > 0 {
        debug!("Cleaned up {} orphaned spool temp files", cleaned);
    }
}
