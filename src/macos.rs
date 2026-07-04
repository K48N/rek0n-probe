use crate::adapter::{normalize_gpu_name, GpuAdapterIdentity};

use std::ffi::{c_char, c_void};

#[link(name = "objc")]
extern "C" {
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn objc_msgSend();
    fn objc_release(obj: *mut c_void);
}

#[link(name = "Metal", kind = "framework")]
extern "C" {
    fn MTLCopyAllDevices() -> *mut c_void;
}

pub fn get_platform_vram(identity: Option<&GpuAdapterIdentity>) -> u64 {
    let devices = metal_devices();
    if devices.is_empty() {
        eprintln!("rek0n-probe: MTLCopyAllDevices returned no devices, assuming 0 bytes VRAM");
        return 0;
    }

    if let Some(identity) = identity {
        for device in &devices {
            if identity.matches_name(&device_name(*device)) {
                return device_budget(*device);
            }
        }
        eprintln!(
            "rek0n-probe: no Metal device matched wgpu selection ({})",
            identity.name
        );
        return 0;
    }

    best_device_budget(&devices)
}

struct MetalDeviceGuard(*mut c_void);

impl Drop for MetalDeviceGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { objc_release(self.0) };
        }
    }
}

fn metal_devices() -> Vec<MetalDeviceGuard> {
    let array = unsafe { MTLCopyAllDevices() };
    if array.is_null() {
        return Vec::new();
    }

    let count: usize = unsafe { msg_send_usize(array, "count") };
    let mut devices = Vec::with_capacity(count);
    for index in 0..count {
        let device = unsafe { msg_send_id(array, "objectAtIndex:", index as u64) };
        if !device.is_null() {
            devices.push(MetalDeviceGuard(device));
        }
    }

    unsafe { objc_release(array) };
    devices
}

fn device_name(device: *mut c_void) -> String {
    let name = unsafe { msg_send_id0(device, "name") };
    if name.is_null() {
        return String::new();
    }
    c_string_from_nsstring(name).unwrap_or_default()
}

fn device_budget(device: *mut c_void) -> u64 {
    unsafe { msg_send_u64(device, "recommendedMaxWorkingSetSize") }
}

fn best_device_budget(devices: &[MetalDeviceGuard]) -> u64 {
    devices
        .iter()
        .map(|device| device_budget(device.0))
        .max()
        .unwrap_or(0)
}

fn c_string_from_nsstring(nsstring: *mut c_void) -> Option<String> {
    let utf8 = unsafe { msg_send_id(nsstring, "UTF8String") } as *const c_char;
    if utf8.is_null() {
        return None;
    }
    Some(
        unsafe { std::ffi::CStr::from_ptr(utf8) }
            .to_string_lossy()
            .into_owned(),
    )
}

unsafe fn msg_send_id0(receiver: *mut c_void, selector: &str) -> *mut c_void {
    let sel = sel_registerName(format!("{selector}\0").as_ptr() as *const c_char);
    let fn_ptr: unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
        std::mem::transmute(objc_msgSend as *const ());
    fn_ptr(receiver, sel)
}

unsafe fn msg_send_id_index(receiver: *mut c_void, selector: &str, index: u64) -> *mut c_void {
    let sel = sel_registerName(format!("{selector}\0").as_ptr() as *const c_char);
    let fn_ptr: unsafe extern "C" fn(*mut c_void, *mut c_void, u64) -> *mut c_void =
        std::mem::transmute(objc_msgSend as *const ());
    fn_ptr(receiver, sel, index)
}

unsafe fn msg_send_usize(receiver: *mut c_void, selector: &str) -> usize {
    let sel = sel_registerName(format!("{selector}\0").as_ptr() as *const c_char);
    let fn_ptr: unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize =
        std::mem::transmute(objc_msgSend as *const ());
    fn_ptr(receiver, sel)
}

unsafe fn msg_send_u64(receiver: *mut c_void, selector: &str) -> u64 {
    let sel = sel_registerName(format!("{selector}\0").as_ptr() as *const c_char);
    let fn_ptr: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u64 =
        std::mem::transmute(objc_msgSend as *const ());
    fn_ptr(receiver, sel)
}

fn msg_send_id(receiver: *mut c_void, selector: &str, index: u64) -> *mut c_void {
    unsafe { msg_send_id_index(receiver, selector, index) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_gpu_name_matches_metal_style_names() {
        let left = normalize_gpu_name("Apple M2 Pro");
        let right = normalize_gpu_name("apple m2 pro");
        assert_eq!(left, right);
    }
}
