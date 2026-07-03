use rek0n_probe::{
    get_platform_vram, preferred_wgpu_adapter, select_execution_backend, BackendRequirement,
    ExecutionBackend, VramMonitor,
};
use std::time::Duration;

const MODEL_VRAM_BYTES: u64 = 4_000_000_000;

fn main() {
    print_adapter();
    print_vram();

    let backend = select_execution_backend(BackendRequirement {
        required_vram_bytes: MODEL_VRAM_BYTES,
    });

    dispatch(backend);

    let monitor = VramMonitor::new(Duration::from_secs(30));
    println!(
        "monitor: {} bytes available (re-probes every {}s)",
        monitor.available_bytes(),
        monitor.interval().as_secs()
    );
}

fn print_adapter() {
    match preferred_wgpu_adapter() {
        Some(adapter) => println!(
            "adapter: {} ({:04x}:{:04x}, {:?})",
            adapter.name, adapter.vendor, adapter.device, adapter.device_type
        ),
        None => println!("adapter: none"),
    }
}

fn print_vram() {
    println!("available vram: {} bytes", get_platform_vram());
}

fn dispatch(backend: ExecutionBackend) {
    match backend {
        ExecutionBackend::WebGpuBurn => run_webgpu_path(),
        ExecutionBackend::CpuLlamaCpp => run_cpu_path(),
    }
}

fn run_webgpu_path() {
    println!("route: WebGPU (burn + wgpu)");
}

fn run_cpu_path() {
    println!("route: CPU (llama.cpp)");
}
