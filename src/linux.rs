use crate::adapter::GpuAdapterIdentity;

use libloading::{Library, Symbol};
use std::ffi::{c_void, CString};
use std::fs;
use std::os::raw::{c_int, c_uint};
use std::path::{Path, PathBuf};

const SYSFS_VRAM_ATTRS: &[(&str, &str)] = &[
    ("mem_info_vram_total", "mem_info_vram_used"),
    ("memory_info/vram_total", "memory_info/vram_used"),
];

const DRM_IOCTL_BASE: u8 = 0x40;
const DRM_XE_DEVICE_QUERY: u8 = 0x00;
const DRM_XE_DEVICE_QUERY_MEM_REGIONS: u32 = 1;
const DRM_XE_MEM_REGION_CLASS_VRAM: u16 = 1;

type NvmlDevice = *mut c_void;

#[repr(C)]
struct NvmlMemory {
    total: u64,
    free: u64,
    used: u64,
}

#[repr(C)]
struct NvmlPciInfo {
    bus_id: [u8; 32],
    domain: c_uint,
    bus: c_uint,
    device: c_uint,
    pci_device_id: c_uint,
    pci_subsystem_id: c_uint,
    reserved: [c_uint; 4],
}

#[repr(C)]
struct DrmXeDeviceQuery {
    extensions: u64,
    query: u32,
    size: u32,
    data: u64,
    reserved: [u64; 2],
}

#[repr(C)]
struct DrmXeMemRegion {
    mem_class: u16,
    instance: u16,
    min_page_size: u32,
    total_size: u64,
    used: u64,
    cpu_visible_size: u64,
    cpu_visible_used: u64,
    reserved: [u64; 6],
}

struct GpuVramCandidate {
    available_bytes: u64,
    discrete: bool,
}

const NVML_SUCCESS: c_int = 0;

type NvmlInitV2Fn = unsafe extern "C" fn() -> c_int;
type NvmlShutdownFn = unsafe extern "C" fn() -> c_int;
type NvmlDeviceGetCountV2Fn = unsafe extern "C" fn(count: *mut c_uint) -> c_int;
type NvmlDeviceGetHandleByIndexV2Fn =
    unsafe extern "C" fn(index: c_uint, device: *mut NvmlDevice) -> c_int;
type NvmlDeviceGetMemoryInfoFn =
    unsafe extern "C" fn(device: NvmlDevice, memory: *mut NvmlMemory) -> c_int;
type NvmlDeviceGetPciInfoFn =
    unsafe extern "C" fn(device: NvmlDevice, pci: *mut NvmlPciInfo) -> c_int;

fn drm_ioctl_xe_device_query() -> libc::c_ulong {
    const IOC_READWRITE: u32 = 3;
    const IOC_NRSHIFT: u32 = 0;
    const IOC_TYPESHIFT: u32 = 8;
    const IOC_SIZESHIFT: u32 = 16;
    const IOC_DIRSHIFT: u32 = 30;

    let dir = IOC_READWRITE << IOC_DIRSHIFT;
    let ty = (b'd' as u32) << IOC_TYPESHIFT;
    let nr = (DRM_IOCTL_BASE + DRM_XE_DEVICE_QUERY) as u32;
    let size = std::mem::size_of::<DrmXeDeviceQuery>() as u32;

    (dir | ty | (nr << IOC_NRSHIFT) | (size << IOC_SIZESHIFT)) as libc::c_ulong
}

fn is_drm_card(name: &str) -> bool {
    let Some(suffix) = name.strip_prefix("card") else {
        return false;
    };
    !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit())
}

fn read_pci_ids(device_dir: &Path) -> Option<(u32, u32)> {
    let vendor = parse_sysfs_hex_id(&fs::read_to_string(device_dir.join("vendor")).ok()?)?;
    let device = parse_sysfs_hex_id(&fs::read_to_string(device_dir.join("device")).ok()?)?;
    Some((vendor, device))
}

fn parse_sysfs_hex_id(raw: &str) -> Option<u32> {
    let raw = raw.trim().trim_start_matches("0x");
    u32::from_str_radix(raw, 16).ok()
}

fn is_discrete_gpu(device_dir: &Path) -> bool {
    fs::read_to_string(device_dir.join("class"))
        .map(|class| class.trim() == "0x030200")
        .unwrap_or(false)
}

fn read_sysfs_available(device_dir: &Path) -> Option<u64> {
    for (total_attr, used_attr) in SYSFS_VRAM_ATTRS {
        let Ok(total_contents) = fs::read_to_string(device_dir.join(total_attr)) else {
            continue;
        };
        let Ok(total) = total_contents.trim().parse::<u64>() else {
            continue;
        };

        let used = fs::read_to_string(device_dir.join(used_attr))
            .ok()
            .and_then(|contents| contents.trim().parse::<u64>().ok())
            .unwrap_or(0);

        return Some(total.saturating_sub(used));
    }

    None
}

fn driver_name(device_dir: &Path) -> Option<String> {
    let driver = device_dir.join("driver");
    let name = driver
        .read_link()
        .ok()?
        .file_name()?
        .to_string_lossy()
        .into_owned();
    Some(name)
}

fn drm_card_device_path(card_path: &Path) -> Option<PathBuf> {
    let card_name = card_path.file_name()?.to_string_lossy();
    Some(PathBuf::from(format!("/dev/dri/{card_name}")))
}

fn parse_xe_mem_regions(buffer: &[u8]) -> Option<u64> {
    if buffer.len() < 8 {
        return None;
    }

    let num_regions = u32::from_ne_bytes(buffer[0..4].try_into().ok()?) as usize;
    let region_size = std::mem::size_of::<DrmXeMemRegion>();
    let header_size = 8;
    let mut available = 0u64;

    for index in 0..num_regions {
        let offset = header_size + index * region_size;
        if offset + region_size > buffer.len() {
            break;
        }

        let mem_class = u16::from_ne_bytes(buffer[offset..offset + 2].try_into().ok()?);
        if mem_class != DRM_XE_MEM_REGION_CLASS_VRAM {
            continue;
        }

        let total_offset = offset + 8;
        let used_offset = offset + 16;
        let total = u64::from_ne_bytes(buffer[total_offset..total_offset + 8].try_into().ok()?);
        let used = u64::from_ne_bytes(buffer[used_offset..used_offset + 8].try_into().ok()?);
        available = available.saturating_add(total.saturating_sub(used));
    }

    if available > 0 {
        Some(available)
    } else {
        None
    }
}

fn try_xe_drm_available_vram(card_path: &Path) -> Option<u64> {
    let device_path = drm_card_device_path(card_path)?;
    let path = CString::new(device_path.to_string_lossy().as_bytes()).ok()?;
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY) };
    if fd < 0 {
        return None;
    }

    let ioctl_num = drm_ioctl_xe_device_query();
    let mut query = DrmXeDeviceQuery {
        extensions: 0,
        query: DRM_XE_DEVICE_QUERY_MEM_REGIONS,
        size: 0,
        data: 0,
        reserved: [0; 2],
    };

    if unsafe { libc::ioctl(fd, ioctl_num as _, &mut query) } < 0 {
        unsafe { libc::close(fd) };
        return None;
    }

    let buffer_size = query.size as usize;
    if buffer_size == 0 {
        unsafe { libc::close(fd) };
        return None;
    }

    let mut buffer = vec![0u8; buffer_size];
    query.data = buffer.as_mut_ptr() as u64;
    query.size = buffer_size as u32;

    let result = if unsafe { libc::ioctl(fd, ioctl_num as _, &mut query) } < 0 {
        None
    } else {
        parse_xe_mem_regions(&buffer)
    };

    unsafe { libc::close(fd) };
    result
}

fn nvml_pci_ids(pci_device_id: c_uint) -> (u32, u32) {
    (pci_device_id & 0xFFFF, (pci_device_id >> 16) & 0xFFFF)
}

fn nvml_free_for_adapter(identity: &GpuAdapterIdentity) -> Option<u64> {
    let lib = unsafe { Library::new("libnvidia-ml.so.1") }
        .or_else(|_| unsafe { Library::new("libnvidia-ml.so") })
        .ok()?;

    let nvml_init: Symbol<NvmlInitV2Fn> = unsafe { lib.get(b"nvmlInit_v2\0") }.ok()?;
    if unsafe { nvml_init() } != NVML_SUCCESS {
        return None;
    }

    let free = nvml_match_device_free(&lib, identity);

    if let Ok(nvml_shutdown) = unsafe { lib.get::<NvmlShutdownFn>(b"nvmlShutdown\0") } {
        let _ = unsafe { nvml_shutdown() };
    }

    free
}

fn nvml_match_device_free(lib: &Library, identity: &GpuAdapterIdentity) -> Option<u64> {
    unsafe {
        let get_count: Symbol<NvmlDeviceGetCountV2Fn> = lib.get(b"nvmlDeviceGetCount_v2\0").ok()?;
        let mut device_count: c_uint = 0;
        if get_count(&mut device_count) != NVML_SUCCESS {
            return None;
        }

        let get_handle: Symbol<NvmlDeviceGetHandleByIndexV2Fn> =
            lib.get(b"nvmlDeviceGetHandleByIndex_v2\0").ok()?;
        let get_memory_info: Symbol<NvmlDeviceGetMemoryInfoFn> =
            lib.get(b"nvmlDeviceGetMemoryInfo\0").ok()?;
        let get_pci_info: Symbol<NvmlDeviceGetPciInfoFn> =
            lib.get(b"nvmlDeviceGetPciInfo\0").ok()?;

        for index in 0..device_count {
            let mut device: NvmlDevice = std::ptr::null_mut();
            if get_handle(index, &mut device) != NVML_SUCCESS {
                continue;
            }

            let mut pci = NvmlPciInfo {
                bus_id: [0; 32],
                domain: 0,
                bus: 0,
                device: 0,
                pci_device_id: 0,
                pci_subsystem_id: 0,
                reserved: [0; 4],
            };
            if get_pci_info(device, &mut pci) != NVML_SUCCESS {
                continue;
            }

            let (vendor, device_id) = nvml_pci_ids(pci.pci_device_id);
            if !identity.matches_pci_ids(vendor, device_id) {
                continue;
            }

            let mut memory = NvmlMemory {
                total: 0,
                free: 0,
                used: 0,
            };
            if get_memory_info(device, &mut memory) != NVML_SUCCESS {
                continue;
            }

            return Some(memory.free);
        }

        None
    }
}

fn drm_available_for_pci(vendor: u32, device: u32) -> Option<u64> {
    let entries = fs::read_dir("/sys/class/drm").ok()?;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !is_drm_card(&name) {
            continue;
        }

        let card_path = entry.path();
        let device_dir = card_path.join("device");
        let Some((card_vendor, card_device)) = read_pci_ids(&device_dir) else {
            continue;
        };
        if card_vendor != vendor || card_device != device {
            continue;
        }

        if driver_name(&device_dir).as_deref() == Some("xe") {
            if let Some(available) = try_xe_drm_available_vram(&card_path) {
                return Some(available);
            }
        }

        if let Some(available) = read_sysfs_available(&device_dir) {
            return Some(available);
        }
    }

    None
}

fn collect_fallback_candidates(candidates: &mut Vec<GpuVramCandidate>) {
    let entries = match fs::read_dir("/sys/class/drm") {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !is_drm_card(&name) {
            continue;
        }

        let card_path = entry.path();
        let device_dir = card_path.join("device");
        if read_pci_ids(&device_dir).is_none() {
            continue;
        }
        let discrete = is_discrete_gpu(&device_dir);

        let available = if driver_name(&device_dir).as_deref() == Some("xe") {
            try_xe_drm_available_vram(&card_path).or_else(|| read_sysfs_available(&device_dir))
        } else {
            read_sysfs_available(&device_dir)
        };

        let Some(available) = available else {
            continue;
        };

        candidates.push(GpuVramCandidate {
            available_bytes: available,
            discrete,
        });
    }
}

fn select_preferred_vram(candidates: &[GpuVramCandidate]) -> Option<u64> {
    if candidates.is_empty() {
        return None;
    }

    let pick_best = |discrete_only: bool| {
        candidates
            .iter()
            .filter(|candidate| !discrete_only || candidate.discrete)
            .map(|candidate| candidate.available_bytes)
            .max()
    };

    pick_best(true).or_else(|| pick_best(false))
}

fn probe_for_adapter(identity: &GpuAdapterIdentity) -> Option<u64> {
    if identity.vendor == 0x10DE {
        if let Some(free) = nvml_free_for_adapter(identity) {
            return Some(free);
        }
    }

    drm_available_for_pci(identity.vendor, identity.device)
}

fn fallback_probe() -> u64 {
    let mut candidates = Vec::new();

    if let Some(identity) = crate::adapter::preferred_wgpu_adapter() {
        if identity.vendor == 0x10DE {
            if let Some(free) = nvml_free_for_adapter(&identity) {
                candidates.push(GpuVramCandidate {
                    available_bytes: free,
                    discrete: identity.device_type == crate::adapter::GpuDeviceType::Discrete,
                });
            }
        }
    }

    collect_fallback_candidates(&mut candidates);

    if let Some(available) = select_preferred_vram(&candidates) {
        return available;
    }

    0
}

pub fn get_platform_vram(identity: Option<&GpuAdapterIdentity>) -> u64 {
    if let Some(identity) = identity {
        if let Some(available) = probe_for_adapter(identity) {
            return available;
        }
        eprintln!(
            "rek0n-probe: adapter-specific probe failed for {}, falling back",
            identity.name
        );
    }

    let available = fallback_probe();
    if available == 0 {
        eprintln!("rek0n-probe: GPU memory probe failed, assuming 0 bytes VRAM");
    }
    available
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::GpuDeviceType;

    #[test]
    fn prefers_discrete_gpu_with_less_vram_over_integrated() {
        let candidates = [
            GpuVramCandidate {
                available_bytes: 8_000_000_000,
                discrete: false,
            },
            GpuVramCandidate {
                available_bytes: 4_000_000_000,
                discrete: true,
            },
        ];

        assert_eq!(select_preferred_vram(&candidates), Some(4_000_000_000));
    }

    #[test]
    fn fallback_picks_largest_among_same_class() {
        let candidates = [
            GpuVramCandidate {
                available_bytes: 4_000_000_000,
                discrete: true,
            },
            GpuVramCandidate {
                available_bytes: 16_000_000_000,
                discrete: true,
            },
        ];

        assert_eq!(select_preferred_vram(&candidates), Some(16_000_000_000));
    }

    #[test]
    fn nvml_pci_id_split_matches_wgpu_ids() {
        let (vendor, device) = nvml_pci_ids(0x248410DE);
        assert_eq!(vendor, 0x10DE);
        assert_eq!(device, 0x2484);

        let identity = GpuAdapterIdentity {
            name: "test".into(),
            vendor,
            device,
            device_type: GpuDeviceType::Discrete,
        };
        assert!(identity.matches_pci_ids(vendor, device));
    }
}
