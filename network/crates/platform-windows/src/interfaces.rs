use std::ffi::c_void;
use std::net::Ipv4Addr;

use async_trait::async_trait;
use platform::{InterfaceEnumerator, InterfaceInfo, PlatformError, PlatformResult};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WiFi::{
    WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanOpenHandle, WlanScan,
    WLAN_INTERFACE_INFO_LIST,
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

    async fn ipv4_broadcast_addresses(&self) -> Vec<Ipv4Addr> {
        tokio::task::spawn_blocking(ipv4_broadcast_addresses_blocking).await.unwrap_or_default()
    }
}

/// Ask every Wi-Fi interface to rescan for networks now. Fire-and-forget:
/// Windows finishes the scan in the background over the next few seconds.
pub(crate) fn request_fresh_scan() {
    const WLAN_CLIENT_VERSION: u32 = 2;
    let mut negotiated_version = 0u32;
    let mut handle = HANDLE::default();
    // SAFETY: valid out-pointers for the duration of the call.
    if unsafe { WlanOpenHandle(WLAN_CLIENT_VERSION, None, &mut negotiated_version, &mut handle) } != 0 {
        return;
    }
    // SAFETY: `handle` was just opened; the interface list is freed below
    // and the handle closed after every use.
    unsafe {
        let mut list_ptr: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        if WlanEnumInterfaces(handle, None, &mut list_ptr) == 0 && !list_ptr.is_null() {
            let count = (*list_ptr).dwNumberOfItems as usize;
            let first = (*list_ptr).InterfaceInfo.as_ptr();
            for i in 0..count {
                let guid = (*first.add(i)).InterfaceGuid;
                let _ = WlanScan(handle, &guid, None, None, None);
            }
            WlanFreeMemory(list_ptr as *const c_void);
        }
        let _ = WlanCloseHandle(handle, None);
    }
}

/// Broadcast address of each IPv4 address on every network adapter that's
/// up (loopback excluded), e.g. `192.168.137.255` for a Windows hotspot's
/// own subnet alongside `192.168.1.255` for the home network.
fn ipv4_broadcast_addresses_blocking() -> Vec<Ipv4Addr> {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
        IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};

    const ERROR_BUFFER_OVERFLOW: u32 = 111;
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;

    // Grown until big enough; in practice a couple of attempts at most.
    let mut size: u32 = 16 * 1024;
    let mut buf: Vec<u64> = Vec::new(); // u64 for alignment of the structs written into it
    let mut ok = false;
    for _ in 0..4 {
        buf = vec![0u64; (size as usize).div_ceil(8)];
        // SAFETY: `buf` is at least `size` bytes and suitably aligned;
        // `size` is a valid in/out pointer.
        let ret = unsafe {
            GetAdaptersAddresses(
                AF_INET.0 as u32,
                flags,
                None,
                Some(buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut size,
            )
        };
        if ret == 0 {
            ok = true;
            break;
        }
        if ret != ERROR_BUFFER_OVERFLOW {
            return Vec::new();
        }
    }
    if !ok {
        return Vec::new();
    }

    let mut out = Vec::new();
    // SAFETY: on success Windows filled `buf` with a linked list of
    // adapters (and per-adapter linked lists of addresses) that all live
    // inside `buf`, which outlives this walk.
    unsafe {
        let mut adapter = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !adapter.is_null() {
            let a = &*adapter;
            if a.OperStatus == IfOperStatusUp {
                let mut unicast = a.FirstUnicastAddress;
                while !unicast.is_null() {
                    let u = &*unicast;
                    let sockaddr = u.Address.lpSockaddr;
                    if !sockaddr.is_null() && (*sockaddr).sa_family == AF_INET {
                        let sin = &*(sockaddr as *const SOCKADDR_IN);
                        let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.S_un.S_addr));
                        if let Some(broadcast) = broadcast_for(ip, u.OnLinkPrefixLength) {
                            if !out.contains(&broadcast) {
                                out.push(broadcast);
                            }
                        }
                    }
                    unicast = u.Next;
                }
            }
            adapter = a.Next;
        }
    }
    out
}

fn broadcast_for(ip: Ipv4Addr, prefix_len: u8) -> Option<Ipv4Addr> {
    if ip.is_loopback() || ip.is_unspecified() || prefix_len == 0 || prefix_len >= 31 {
        return None;
    }
    let host_mask = u32::MAX >> prefix_len;
    Some(Ipv4Addr::from(u32::from(ip) | host_mask))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_subnet_broadcast() {
        assert_eq!(broadcast_for(Ipv4Addr::new(192, 168, 137, 1), 24), Some(Ipv4Addr::new(192, 168, 137, 255)));
        assert_eq!(broadcast_for(Ipv4Addr::new(10, 1, 2, 3), 16), Some(Ipv4Addr::new(10, 1, 255, 255)));
        assert_eq!(broadcast_for(Ipv4Addr::new(127, 0, 0, 1), 8), None);
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
