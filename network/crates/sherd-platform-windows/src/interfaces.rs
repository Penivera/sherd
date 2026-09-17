use std::ffi::c_void;

use async_trait::async_trait;
use sherd_platform::{InterfaceEnumerator, InterfaceInfo, PlatformError, PlatformResult};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WiFi::{
    WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanOpenHandle, WLAN_INTERFACE_INFO_LIST,
};

/// Read-only interface listing via the native WlanAPI (`wlanapi.dll`).
/// Kept separate from the `netsh`-based controllers because it's a cheap,
/// synchronous FFI call rather than a spawned process.
pub struct WlanApiInterfaceEnumerator;

#[async_trait]
impl InterfaceEnumerator for WlanApiInterfaceEnumerator {
    async fn list(&self) -> PlatformResult<Vec<InterfaceInfo>> {
        // WlanAPI is blocking FFI; keep it off the async executor threads.
        tokio::task::spawn_blocking(list_interfaces_blocking)
            .await
            .map_err(|e| PlatformError::CommandFailed(format!("WlanAPI task panicked: {e}")))?
    }
}

fn list_interfaces_blocking() -> PlatformResult<Vec<InterfaceInfo>> {
    // Client version 2 = Vista and later (WLAN_API_VERSION_2_0).
    const WLAN_CLIENT_VERSION: u32 = 2;

    let mut negotiated_version = 0u32;
    let mut handle = HANDLE::default();

    // SAFETY: `negotiated_version` and `handle` are valid, live local
    // variables for the duration of this call, matching WlanOpenHandle's
    // out-pointer contract.
    let open_result =
        unsafe { WlanOpenHandle(WLAN_CLIENT_VERSION, None, &mut negotiated_version, &mut handle) };
    if open_result != 0 {
        return Err(PlatformError::CommandFailed(format!(
            "WlanOpenHandle failed with Win32 error {open_result}"
        )));
    }

    // SAFETY: `handle` was just successfully opened above.
    let result = unsafe { enumerate(handle) };

    // SAFETY: `handle` was opened above and is closed exactly once, after
    // every use of it (including inside `enumerate`) has finished.
    unsafe {
        let _ = WlanCloseHandle(handle, None);
    }

    result
}

/// # Safety
/// `handle` must be a WlanAPI client handle successfully returned by
/// `WlanOpenHandle` and not yet closed.
unsafe fn enumerate(handle: HANDLE) -> PlatformResult<Vec<InterfaceInfo>> {
    let mut list_ptr: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();

    // SAFETY: `handle` is valid per this function's contract; `list_ptr`
    // is a valid out-pointer for the duration of the call.
    let enum_result = WlanEnumInterfaces(handle, None, &mut list_ptr);
    if enum_result != 0 {
        return Err(PlatformError::CommandFailed(format!(
            "WlanEnumInterfaces failed with Win32 error {enum_result}"
        )));
    }
    if list_ptr.is_null() {
        return Ok(Vec::new());
    }

    let count = (*list_ptr).dwNumberOfItems as usize;
    // `InterfaceInfo` is declared as a 1-element array standing in for a C
    // flexible array member — the real `count` interfaces are laid out
    // contiguously starting at its address.
    let first = (*list_ptr).InterfaceInfo.as_ptr();
    let mut interfaces = Vec::with_capacity(count);
    for i in 0..count {
        let info = &*first.add(i);
        let nul_at = info
            .strInterfaceDescription
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(info.strInterfaceDescription.len());
        let description = String::from_utf16_lossy(&info.strInterfaceDescription[..nul_at]);
        interfaces.push(InterfaceInfo {
            description,
            id: format!("{:?}", info.InterfaceGuid),
        });
    }

    WlanFreeMemory(list_ptr as *const c_void);
    Ok(interfaces)
}
