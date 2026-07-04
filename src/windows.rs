use crate::adapter::GpuAdapterIdentity;

// Module name collides with the `windows` crate — use `::windows::` for the dependency.
use ::windows::Win32::Graphics::Dxgi::IDXGIAdapter1;

pub fn get_platform_vram(identity: Option<&GpuAdapterIdentity>) -> u64 {
    if let Some(identity) = identity {
        if let Some(budget) = dxgi_budget_for_adapter(identity) {
            return budget;
        }
        eprintln!(
            "rek0n-probe: no DXGI adapter matched wgpu selection ({})",
            identity.name
        );
        return 0;
    }

    dxgi_budget_high_performance(None)
}

fn dxgi_budget_for_adapter(identity: &GpuAdapterIdentity) -> Option<u64> {
    let budget = dxgi_budget_high_performance(Some(identity));
    if budget > 0 {
        Some(budget)
    } else {
        None
    }
}

fn dxgi_budget_high_performance(identity: Option<&GpuAdapterIdentity>) -> u64 {
    use ::windows::core::ComInterface;
    use ::windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIFactory1, IDXGIFactory6, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,
    };

    let factory: IDXGIFactory1 = match unsafe { CreateDXGIFactory1() } {
        Ok(factory) => factory,
        Err(err) => {
            eprintln!("rek0n-probe: CreateDXGIFactory1 failed: {err:?}");
            return 0;
        }
    };

    if let Ok(factory6) = factory.cast::<IDXGIFactory6>() {
        for index in 0u32.. {
            let adapter = match unsafe {
                factory6.EnumAdapterByGpuPreference(index, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE)
            } {
                Ok(adapter) => adapter,
                Err(_) => break,
            };

            if adapter_matches(&adapter, identity) {
                return adapter_local_budget(&adapter);
            }
        }
    }

    fallback_match_adapter(&factory, identity)
}

fn adapter_matches(adapter: &IDXGIAdapter1, identity: Option<&GpuAdapterIdentity>) -> bool {
    let Some(identity) = identity else {
        return true;
    };

    let mut desc = ::windows::Win32::Graphics::Dxgi::DXGI_ADAPTER_DESC1::default();
    if unsafe { adapter.GetDesc1(&mut desc) }.is_err() {
        return false;
    }

    if identity.matches_pci_ids(desc.VendorId, desc.DeviceId) {
        return true;
    }

    let description = String::from_utf16_lossy(&desc.Description);
    identity.matches_name(description.trim_end_matches('\0'))
}

fn adapter_local_budget(adapter: &IDXGIAdapter1) -> u64 {
    use ::windows::core::ComInterface;
    use ::windows::Win32::Graphics::Dxgi::{
        IDXGIAdapter3, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_MEMORY_SEGMENT_GROUP_LOCAL,
        DXGI_QUERY_VIDEO_MEMORY_INFO,
    };

    let mut desc = ::windows::Win32::Graphics::Dxgi::DXGI_ADAPTER_DESC1::default();
    if let Err(err) = unsafe { adapter.GetDesc1(&mut desc) } {
        eprintln!("rek0n-probe: GetDesc1 failed: {err:?}");
        return 0;
    }

    if (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
        return 0;
    }

    let adapter3: IDXGIAdapter3 = match adapter.cast() {
        Ok(adapter3) => adapter3,
        Err(err) => {
            eprintln!("rek0n-probe: IDXGIAdapter3 cast failed: {err:?}");
            return 0;
        }
    };

    let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
    match unsafe { adapter3.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info) } {
        Ok(()) => info.Budget.saturating_sub(info.CurrentUsage),
        Err(err) => {
            eprintln!("rek0n-probe: QueryVideoMemoryInfo failed: {err:?}");
            0
        }
    }
}

fn fallback_match_adapter(
    factory: &::windows::Win32::Graphics::Dxgi::IDXGIFactory1,
    identity: Option<&GpuAdapterIdentity>,
) -> u64 {
    use ::windows::Win32::Graphics::Dxgi::DXGI_ADAPTER_FLAG_SOFTWARE;

    let mut best_discrete = 0u64;
    let mut best_any = 0u64;

    for index in 0u32.. {
        let adapter = match unsafe { factory.EnumAdapters1(index) } {
            Ok(adapter) => adapter,
            Err(_) => break,
        };

        let mut desc = ::windows::Win32::Graphics::Dxgi::DXGI_ADAPTER_DESC1::default();
        if unsafe { adapter.GetDesc1(&mut desc) }.is_err() {
            continue;
        }

        if (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0 {
            continue;
        }

        if !adapter_matches(&adapter, identity) {
            continue;
        }

        let budget = adapter_local_budget(&adapter);
        best_any = best_any.max(budget);
        if desc.DedicatedVideoMemory > 0 {
            best_discrete = best_discrete.max(budget);
        }
    }

    if best_discrete > 0 {
        best_discrete
    } else {
        best_any
    }
}
