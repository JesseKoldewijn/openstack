use std::path::PathBuf;

use openstack_integration_tests::benchmark::{BenchmarkConfig, run_profile};

fn parse_args() -> (String, Option<PathBuf>) {
    let mut profile = "all-services-smoke".to_string();
    let mut output: Option<PathBuf> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--profile" {
            if let Some(value) = args.next() {
                profile = value;
            }
            continue;
        }

        if arg == "--output" {
            if let Some(value) = args.next() {
                output = Some(PathBuf::from(value));
            }
            continue;
        }

        if !arg.starts_with('-') {
            profile = arg;
        }
    }

    (profile, output)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (profile, output) = parse_args();
    let config = BenchmarkConfig::default();
    let report = run_profile(&config, &profile, output).await?;

    println!(
        "benchmark profile '{}' complete: {} scenarios, openstack errors={}, localstack errors={}",
        report.profile,
        report.summary.total_scenarios,
        report.summary.openstack_error_count,
        report.summary.localstack_error_count
    );

    Ok(())
}
