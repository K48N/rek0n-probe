# rek0n-probe

Part of [rek0n](https://github.com/K48N/rek0n). Probes GPU memory and picks WebGPU or CPU for local inference.

## Overview

Before rek0n loads a model, this crate checks whether the selected GPU has enough free memory for the job. It returns `WebGpuBurn` or `CpuLlamaCpp`, or raw available bytes through `get_platform_vram()`. `VramMonitor` caches readings for long-running daemons.

Generic system memory crates answer a different question. rek0n needs free memory on the specific adapter wgpu already chose, measured with OS-specific APIs.

## How it works

1. `wgpu::request_adapter(HighPerformance)` picks the GPU rek0n will bind to. `VramMonitor::refresh()` re-probes that adapter on each refresh cycle.
2. Platform code reads **available** memory for that adapter:
   - **Windows**: DXGI `QueryVideoMemoryInfo`, budget minus current usage
   - **macOS**: enumerate Metal devices with `MTLCopyAllDevices`, match by normalized device name, read `recommendedMaxWorkingSetSize`
   - **Linux**: NVML, sysfs, or xe DRM ioctls with PCI matching where possible
3. If wgpu identity is known but OS matching fails, the probe returns zero and routes to CPU instead of guessing a different card.
4. `VramAvailability::Unknown` on unsupported hosts also routes to CPU.

## Design

**wgpu selects, OS APIs measure.** Picking the largest card in the system breaks on dual-GPU and hybrid laptops.

**Available memory, not sticker VRAM.** Budget minus usage reflects what the process can actually allocate now.

**Fail closed.** Running on the wrong GPU is worse than falling back to CPU.

**Small FFI surface.** Hand-written platform probes plus the `windows` crate. wgpu is the only large shared dependency.

## Usage

```rust
use rek0n_probe::{select_execution_backend, BackendRequirement, VramMonitor};

let backend = select_execution_backend(BackendRequirement {
    required_vram_bytes: 4_000_000_000,
});

let monitor = VramMonitor::default();
let backend = monitor.select_backend(BackendRequirement {
    required_vram_bytes: 4_000_000_000,
});
```

Example:

```sh
cargo run --example route_workload
```

## Known gaps

- Intel xe needs a recent kernel; i915 discrete uses sysfs heuristics.
- Headless CI reports zero bytes and routes to CPU.
- macOS matching is name-based because Metal does not expose PCI ids the way DXGI does.
- No Apple Silicon unified-memory split between CPU and GPU beyond Metal budgets.

## License

MIT
