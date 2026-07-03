use std::time::Duration;

use rek0n_probe::testing::sample_discrete_gpu;
use rek0n_probe::{BackendRequirement, ExecutionBackend, VramMonitor};

#[test]
fn monitor_selects_backend_from_injected_vram() {
    let monitor = VramMonitor::from_available_bytes(
        Some(sample_discrete_gpu()),
        8_000_000_000,
        Duration::from_secs(3600),
    );

    assert_eq!(
        monitor.select_backend(BackendRequirement {
            required_vram_bytes: 4_000_000_000,
        }),
        ExecutionBackend::WebGpuBurn
    );
}

#[test]
fn monitor_refresh_picks_up_injected_vram_change() {
    let monitor = VramMonitor::from_available_bytes(
        Some(sample_discrete_gpu()),
        8_000_000_000,
        Duration::from_secs(3600),
    );

    monitor.set_available_bytes(500_000_000);
    assert_eq!(monitor.refresh(), 500_000_000);
    assert_eq!(
        monitor.select_backend(BackendRequirement {
            required_vram_bytes: 4_000_000_000,
        }),
        ExecutionBackend::CpuLlamaCpp
    );
}

#[test]
fn monitor_keeps_cached_vram_within_interval() {
    let monitor = VramMonitor::from_available_bytes(
        Some(sample_discrete_gpu()),
        8_000_000_000,
        Duration::from_secs(3600),
    );

    assert_eq!(monitor.available_bytes(), 8_000_000_000);
    monitor.set_available_bytes(1);
    assert_eq!(monitor.available_bytes(), 8_000_000_000);
}

#[test]
fn monitor_exposes_injected_adapter_identity() {
    let adapter = sample_discrete_gpu();
    let monitor = VramMonitor::from_available_bytes(
        Some(adapter.clone()),
        8_000_000_000,
        Duration::from_secs(3600),
    );

    assert_eq!(monitor.adapter(), Some(adapter));
}

#[test]
fn headless_monitor_fixture_routes_to_cpu() {
    let monitor = VramMonitor::from_available_bytes(
        Some(sample_discrete_gpu()),
        0,
        Duration::from_secs(3600),
    );

    assert_eq!(
        monitor.select_backend(BackendRequirement {
            required_vram_bytes: 1,
        }),
        ExecutionBackend::CpuLlamaCpp
    );
}
