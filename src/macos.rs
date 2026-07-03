use crate::adapter::GpuAdapterIdentity;

use std::ffi::c_void;
use std::os::raw::c_char;

#[link(name = "objc")]
extern "C" {
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn objc_msgSend(receiver: *mut c_void, selector: *mut c_void) -> u64;
    fn objc_release(obj: *mut c_void);
}

#[link(name = "Metal", kind = "framework")]
extern "C" {
    fn MTLCreateSystemDefaultDevice() -> *mut c_void;
}

pub fn get_platform_vram(_identity: Option<&GpuAdapterIdentity>) -> u64 {
    let device = unsafe { MTLCreateSystemDefaultDevice() };
    if device.is_null() {
        eprintln!("rek0n-probe: MTLCreateSystemDefaultDevice returned NULL, assuming 0 bytes VRAM");
        return 0;
    }

    let recommended_bytes = unsafe {
        let selector =
            sel_registerName(b"recommendedMaxWorkingSetSize\0".as_ptr() as *const c_char);
        objc_msgSend(device, selector)
    };

    unsafe { objc_release(device) };

    recommended_bytes
}
