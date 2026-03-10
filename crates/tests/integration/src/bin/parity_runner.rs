use openstack_integration_tests::parity::{ParityConfig, run_profile};

fn parse_args() -> String {
    let mut profile = "core".to_string();
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        if arg == "--profile" {
            if let Some(value) = args.next() {
                profile = value;
            }
            continue;
        }

        if !arg.starts_with('-') {
            profile = arg;
        }
    }

    profile
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let profile = parse_args();
    let config = ParityConfig::default();
    let report = run_profile(&config, &profile).await?;

    println!(
        "parity profile '{}' complete: {}/{} passed ({} failed), accepted differences: {}",
        report.profile,
        report.summary.passed,
        report.summary.total_scenarios,
        report.summary.failed,
        report.summary.accepted_differences
    );

    if report.summary.failed > 0 {
        std::process::exit(1);
    }

    Ok(())
}
