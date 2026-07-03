# rek0n-probe

Part of my secret lil project called rek0n. Probes GPU memory and decides whether the local daemon runs on WebGPU or falls back to CPU.

## What it is

A Rust library that runs before model load: given a VRAM requirement, it returns `WebGpuBurn` or `CpuLlamaCpp`, or raw available bytes via `get_platform_vram()`. `VramMonitor` adds cached re-probing for long-running daemons.

Crates like sysinfo and nvml-wrapper already read system memory, and for most tools they are the right call. rek0n does not lean on them here because the real question is not total VRAM, it is how much the specific adapter wgpu already selected has free, on whatever OS the daemon happens to be running on. That is narrower and more OS-specific than those crates are built for, so this crate talks to DXGI, NVML, and sysfs directly instead.

## How it works

1. Call `wgpu::request_adapter(HighPerformance)` to get the GPU rek0n will bind to.
2. Query that adapter's **available** memory by PCI vendor/device:
   - **Windows**: DXGI `QueryVideoMemoryInfo` budget
   - **macOS**: Metal `recommendedMaxWorkingSetSize`
   - **Linux**: NVML free (NVIDIA), sysfs total minus used (AMD/i915), xe DRM ioctl (Intel Arc)
3. Compare against the requirement. Any failure returns `0` and routes to CPU.

## Why it's built this way

**wgpu selects the adapter; OS APIs measure it.** These are separate problems. wgpu knows which GPU it will use. DXGI, NVML, and sysfs know how much memory that GPU has free. Picking "largest VRAM" breaks on dual-GPU and hybrid laptops.

**Minimal measurement stack.** No sysinfo, nvml-wrapper, or metal crate: hand-written FFI plus the `windows` crate. wgpu is the only heavy dependency; rek0n needs it anyway.

**Available memory, not capacity.** Budgets and free bytes reflect what the process can use now, not sticker VRAM.

**Fail closed.** Wrong backend on GPU is worse than running on CPU.

## Shortcomings

- Intel xe needs kernel 6.9+; i915 discrete uses sysfs.
- Headless / CI with no GPU returns `0`, and that is intentional.
- `VramMonitor` locks adapter identity at creation; hot-plug needs a new monitor.
- If PCI matching fails, the Linux fallback still heuristically picks among cards.

## Usage

```rust
use rek0n_probe::{select_execution_backend, BackendRequirement};

let backend = select_execution_backend(BackendRequirement {
    required_vram_bytes: 4_000_000_000,
});
```

See `examples/route_workload.rs` for a full WebGPU vs CPU dispatch flow:

```sh
cargo run --example route_workload
```

## License

MIT
