use openstack_integration_tests::parity::{ParityConfig, run_profile};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let profile = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "core".to_string());
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
