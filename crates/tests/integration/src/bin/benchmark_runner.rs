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
    println!(
        "required role coverage gaps: {}",
        report.summary.missing_required_role_count
    );
    println!(
        "lane mode: {:?}; execution driver: {:?}; persistence modes: openstack={:?}, localstack={:?}, equivalent={}",
        report.runtime.benchmark_lane_mode,
        report.runtime.execution_driver,
        report.runtime.openstack_persistence_mode,
        report.runtime.localstack_persistence_mode,
        report.runtime.persistence_mode_equivalent
    );

    if let Some(v) = report.summary.avg_latency_p95_ratio {
        println!("average p95 ratio (OS/LS): {v:.3}");
    }
    if let Some(v) = report.summary.avg_throughput_ratio {
        println!("average throughput ratio (OS/LS): {v:.3}");
    }
    if let Some(memory) = &report.memory_summary {
        let os_idle_mb = memory
            .openstack_idle_rss_bytes
            .map(|b| b as f64 / (1024.0 * 1024.0))
            .map(|v| format!("{v:.2}"))
            .unwrap_or_else(|| "n/a".to_string());
        let ls_idle_mb = memory
            .localstack_idle_rss_bytes
            .map(|b| b as f64 / (1024.0 * 1024.0))
            .map(|v| format!("{v:.2}"))
            .unwrap_or_else(|| "n/a".to_string());
        let os_mb = memory
            .openstack_rss_bytes
            .map(|b| b as f64 / (1024.0 * 1024.0))
            .map(|v| format!("{v:.2}"))
            .unwrap_or_else(|| "n/a".to_string());
        let ls_mb = memory
            .localstack_rss_bytes
            .map(|b| b as f64 / (1024.0 * 1024.0))
            .map(|v| format!("{v:.2}"))
            .unwrap_or_else(|| "n/a".to_string());
        let ratio = memory
            .rss_ratio_openstack_over_localstack
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "n/a".to_string());
        println!(
            "memory rss (MB): openstack idle={}, openstack post-load={}, localstack idle={}, localstack post-load={}, os/ls ratio={}",
            os_idle_mb, os_mb, ls_idle_mb, ls_mb, ratio
        );
        if !memory.missing_targets.is_empty() {
            println!(
                "memory diagnostics: missing targets={}",
                memory.missing_targets.join(",")
            );
        }
    }
    if let Some(startup) = &report.startup_summary {
        let os = startup
            .openstack_avg_ms
            .map(|v| format!("{v:.2}"))
            .unwrap_or_else(|| "n/a".to_string());
        let ls = startup
            .localstack_avg_ms
            .map(|v| format!("{v:.2}"))
            .unwrap_or_else(|| "n/a".to_string());
        let ratio = startup
            .startup_ratio_openstack_over_localstack
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "n/a".to_string());
        println!(
            "startup avg (ms): openstack={}, localstack={}, os/ls ratio={}",
            os, ls, ratio
        );
        if !startup.missing_targets.is_empty() {
            println!(
                "startup diagnostics: missing targets={}",
                startup.missing_targets.join(",")
            );
        }
    }

    println!("per-service comparison:");
    for (service, summary) in &report.summary.per_service {
        let p95 = summary
            .avg_latency_p95_ratio
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "n/a".to_string());
        let p99 = summary
            .avg_latency_p99_ratio
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "n/a".to_string());
        let throughput = summary
            .avg_throughput_ratio
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "n/a".to_string());
        println!(
            "  - {service}: class={:?}, durability={:?}, scenarios={}, skipped={}, missing_roles={}, p95_ratio={}, p99_ratio={}, throughput_ratio={}",
            summary.service_execution_class,
            summary.service_durability_class,
            summary.total_scenarios,
            summary.skipped_scenarios,
            summary
                .missing_roles
                .iter()
                .map(|role| format!("{:?}", role).to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(","),
            p95,
            p99,
            throughput
        );
        if !summary.class_envelope_breaches.is_empty() {
            println!(
                "    envelope breaches: {}",
                summary.class_envelope_breaches.join(", ")
            );
        }
    }

    Ok(())
}
