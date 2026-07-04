//! Cross-platform GPU memory probing for [rek0n](https://github.com/K48N/rek0n) backend selection.

mod adapter;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;

use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub use adapter::{preferred_wgpu_adapter, GpuAdapterIdentity, GpuDeviceType};

mod sys_probe {
    #[cfg(target_os = "macos")]
    pub(crate) use crate::macos::get_platform_vram;

    #[cfg(target_os = "windows")]
    pub(crate) use crate::windows::get_platform_vram;

    #[cfg(target_os = "linux")]
    pub(crate) use crate::linux::get_platform_vram;

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    pub(crate) fn get_platform_vram(_identity: Option<&crate::GpuAdapterIdentity>) -> u64 {
        0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VramAvailability {
    Unknown,
    Bytes(u64),
}

impl VramAvailability {
    pub fn as_bytes(self) -> u64 {
        match self {
            Self::Unknown => 0,
            Self::Bytes(bytes) => bytes,
        }
    }

    pub fn is_unknown(self) -> bool {
        matches!(self, Self::Unknown)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionBackend {
    WebGpuBurn,
    CpuLlamaCpp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendRequirement {
    pub required_vram_bytes: u64,
}

struct MonitorState {
    adapter: Option<GpuAdapterIdentity>,
    cached_bytes: u64,
    cached_at: Option<Instant>,
    fixed_available: Option<u64>,
}

pub struct VramMonitor {
    interval: Duration,
    state: Mutex<MonitorState>,
}

impl Default for VramMonitor {
    fn default() -> Self {
        Self::new(Duration::from_secs(30))
    }
}

impl VramMonitor {
    pub fn new(interval: Duration) -> Self {
        let interval = interval.max(Duration::from_secs(1));
        Self {
            interval,
            state: Mutex::new(MonitorState {
                adapter: preferred_wgpu_adapter(),
                cached_bytes: 0,
                cached_at: None,
                fixed_available: None,
            }),
        }
    }

    #[doc(hidden)]
    pub fn from_available_bytes(
        adapter: Option<GpuAdapterIdentity>,
        available_bytes: u64,
        interval: Duration,
    ) -> Self {
        Self {
            interval,
            state: Mutex::new(MonitorState {
                adapter,
                cached_bytes: available_bytes,
                cached_at: Some(Instant::now()),
                fixed_available: Some(available_bytes),
            }),
        }
    }
}

fn lock_state(state: &Mutex<MonitorState>) -> MutexGuard<'_, MonitorState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl VramMonitor {
    #[doc(hidden)]
    pub fn set_available_bytes(&self, available_bytes: u64) {
        let mut state = lock_state(&self.state);
        state.fixed_available = Some(available_bytes);
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    pub fn adapter(&self) -> Option<GpuAdapterIdentity> {
        let mut state = lock_state(&self.state);
        if state
            .cached_at
            .is_none_or(|cached_at| cached_at.elapsed() >= self.interval)
        {
            refresh_state(&mut state);
        }
        state.adapter.clone()
    }

    pub fn available_bytes(&self) -> u64 {
        let mut state = lock_state(&self.state);
        if state
            .cached_at
            .is_none_or(|cached_at| cached_at.elapsed() >= self.interval)
        {
            refresh_state(&mut state);
        }
        state.cached_bytes
    }

    pub fn refresh(&self) -> u64 {
        let mut state = lock_state(&self.state);
        refresh_state(&mut state);
        state.cached_bytes
    }

    pub fn probe(&self) -> VramAvailability {
        let mut state = lock_state(&self.state);
        refresh_state(&mut state);
        state
            .fixed_available
            .map(VramAvailability::Bytes)
            .unwrap_or_else(|| probe_vram(state.adapter.as_ref()))
    }

    pub fn select_backend(&self, requirements: BackendRequirement) -> ExecutionBackend {
        decide_backend_availability(self.probe(), requirements.required_vram_bytes)
    }
}

pub fn probe_platform_vram() -> VramAvailability {
    probe_vram(preferred_wgpu_adapter().as_ref())
}

pub fn get_platform_vram() -> u64 {
    probe_platform_vram().as_bytes()
}

fn probe_vram(identity: Option<&GpuAdapterIdentity>) -> VramAvailability {
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = identity;
        return VramAvailability::Unknown;
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        VramAvailability::Bytes(sys_probe::get_platform_vram(identity))
    }
}

fn refresh_state(state: &mut MonitorState) {
    state.adapter = preferred_wgpu_adapter();
    state.cached_bytes = state
        .fixed_available
        .map(VramAvailability::Bytes)
        .unwrap_or_else(|| probe_vram(state.adapter.as_ref()))
        .as_bytes();
    state.cached_at = Some(Instant::now());
}

pub fn select_execution_backend(requirements: BackendRequirement) -> ExecutionBackend {
    decide_backend_availability(probe_platform_vram(), requirements.required_vram_bytes)
}

fn decide_backend(available_vram_bytes: u64, required_vram_bytes: u64) -> ExecutionBackend {
    decide_backend_availability(
        VramAvailability::Bytes(available_vram_bytes),
        required_vram_bytes,
    )
}

fn decide_backend_availability(
    availability: VramAvailability,
    required_vram_bytes: u64,
) -> ExecutionBackend {
    match availability {
        VramAvailability::Unknown => ExecutionBackend::CpuLlamaCpp,
        VramAvailability::Bytes(bytes) if bytes >= required_vram_bytes => {
            ExecutionBackend::WebGpuBurn
        }
        VramAvailability::Bytes(_) => ExecutionBackend::CpuLlamaCpp,
    }
}

#[doc(hidden)]
pub mod testing {
    use super::*;

    pub fn select_backend_for_vram(
        available_vram_bytes: u64,
        requirements: BackendRequirement,
    ) -> ExecutionBackend {
        decide_backend(available_vram_bytes, requirements.required_vram_bytes)
    }

    pub fn route_workload(
        adapter: &GpuAdapterIdentity,
        available_vram_bytes: u64,
        requirements: BackendRequirement,
    ) -> ExecutionBackend {
        let _ = adapter;
        select_backend_for_vram(available_vram_bytes, requirements)
    }

    pub fn sample_discrete_gpu() -> GpuAdapterIdentity {
        GpuAdapterIdentity {
            name: "NVIDIA GeForce RTX 4090".into(),
            vendor: 0x10DE,
            device: 0x2484,
            device_type: GpuDeviceType::Discrete,
        }
    }

    pub fn headless_ci_fixture() -> (GpuAdapterIdentity, u64) {
        (sample_discrete_gpu(), 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_webgpu_when_available_exceeds_requirement() {
        assert_eq!(
            decide_backend(8_000_000_000, 4_000_000_000),
            ExecutionBackend::WebGpuBurn
        );
    }

    #[test]
    fn selects_webgpu_when_available_exactly_matches_requirement() {
        assert_eq!(
            decide_backend(4_000_000_000, 4_000_000_000),
            ExecutionBackend::WebGpuBurn
        );
    }

    #[test]
    fn selects_cpu_when_available_is_below_requirement() {
        assert_eq!(
            decide_backend(1_000_000_000, 4_000_000_000),
            ExecutionBackend::CpuLlamaCpp
        );
    }

    #[test]
    fn selects_cpu_when_probe_returns_zero() {
        assert_eq!(decide_backend(0, 1), ExecutionBackend::CpuLlamaCpp);
    }

    #[test]
    fn unknown_availability_routes_to_zero_bytes() {
        assert_eq!(VramAvailability::Unknown.as_bytes(), 0);
        assert!(VramAvailability::Unknown.is_unknown());
    }

    #[test]
    fn unknown_availability_routes_to_cpu() {
        assert_eq!(
            decide_backend_availability(VramAvailability::Unknown, 0),
            ExecutionBackend::CpuLlamaCpp
        );
    }

    #[test]
    fn monitor_default_interval_is_30_seconds() {
        assert_eq!(VramMonitor::default().interval(), Duration::from_secs(30));
    }
}
