use wgpu::{DeviceType, PowerPreference};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuAdapterIdentity {
    pub name: String,
    pub vendor: u32,
    pub device: u32,
    pub device_type: GpuDeviceType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuDeviceType {
    Discrete,
    Integrated,
    Other,
}

pub fn preferred_wgpu_adapter() -> Option<GpuAdapterIdentity> {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))?;

    Some(GpuAdapterIdentity::from_wgpu_info(&adapter.get_info()))
}

impl GpuAdapterIdentity {
    fn from_wgpu_info(info: &wgpu::AdapterInfo) -> Self {
        Self {
            name: info.name.clone(),
            vendor: info.vendor,
            device: info.device,
            device_type: match info.device_type {
                DeviceType::DiscreteGpu => GpuDeviceType::Discrete,
                DeviceType::IntegratedGpu => GpuDeviceType::Integrated,
                _ => GpuDeviceType::Other,
            },
        }
    }

    pub fn matches_pci_ids(&self, vendor: u32, device: u32) -> bool {
        self.vendor == vendor && self.device == device
    }

    pub fn matches_name(&self, other: &str) -> bool {
        normalize_gpu_name(&self.name) == normalize_gpu_name(other)
    }
}

pub(crate) fn normalize_gpu_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pci_id_matching_is_exact() {
        let identity = GpuAdapterIdentity {
            name: "Test GPU".into(),
            vendor: 0x10DE,
            device: 0x2484,
            device_type: GpuDeviceType::Discrete,
        };

        assert!(identity.matches_pci_ids(0x10DE, 0x2484));
        assert!(!identity.matches_pci_ids(0x10DE, 0x1234));
    }

    #[test]
    fn name_matching_is_case_insensitive() {
        let identity = GpuAdapterIdentity {
            name: "NVIDIA GeForce RTX 4090".into(),
            vendor: 0,
            device: 0,
            device_type: GpuDeviceType::Discrete,
        };

        assert!(identity.matches_name("nvidia geforce rtx 4090"));
    }
}
