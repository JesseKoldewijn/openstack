use anyhow::Result;
use tracing::info;

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

    // Register all built-in service providers
    register_services(&plugin_manager, &config);

    // Initialize state
    let state_manager = openstack_state::StateManager::new(config.clone());
    state_manager.load_on_startup().await?;

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
) {
    let services = &config.services;

    macro_rules! register {
        ($name:literal, $provider:expr) => {
            if services.is_enabled($name) {
                manager.register($name, $provider);
            }
        };
    }

    register!("s3", openstack_s3::S3Provider::new());
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
}
