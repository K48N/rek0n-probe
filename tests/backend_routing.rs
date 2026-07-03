use rek0n_probe::testing::{headless_ci_fixture, route_workload, sample_discrete_gpu};
use rek0n_probe::{BackendRequirement, ExecutionBackend};

#[test]
fn headless_ci_fixture_routes_to_cpu() {
    let (adapter, available) = headless_ci_fixture();
    let backend = route_workload(
        &adapter,
        available,
        BackendRequirement {
            required_vram_bytes: 1,
        },
    );

    assert_eq!(backend, ExecutionBackend::CpuLlamaCpp);
}

#[test]
fn sufficient_vram_routes_to_webgpu() {
    let adapter = sample_discrete_gpu();
    let backend = route_workload(
        &adapter,
        8_000_000_000,
        BackendRequirement {
            required_vram_bytes: 4_000_000_000,
        },
    );

    assert_eq!(backend, ExecutionBackend::WebGpuBurn);
}

#[test]
fn insufficient_vram_routes_to_cpu() {
    let adapter = sample_discrete_gpu();
    let backend = route_workload(
        &adapter,
        1_000_000_000,
        BackendRequirement {
            required_vram_bytes: 4_000_000_000,
        },
    );

    assert_eq!(backend, ExecutionBackend::CpuLlamaCpp);
}

#[test]
fn zero_requirement_routes_to_webgpu_on_headless_fixture() {
    let (adapter, available) = headless_ci_fixture();
    let backend = route_workload(
        &adapter,
        available,
        BackendRequirement {
            required_vram_bytes: 0,
        },
    );

    assert_eq!(backend, ExecutionBackend::WebGpuBurn);
}

#[test]
fn exact_vram_match_routes_to_webgpu() {
    let adapter = sample_discrete_gpu();
    let backend = route_workload(
        &adapter,
        4_000_000_000,
        BackendRequirement {
            required_vram_bytes: 4_000_000_000,
        },
    );

    assert_eq!(backend, ExecutionBackend::WebGpuBurn);
}
