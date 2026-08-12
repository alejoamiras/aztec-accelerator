//! Who actually owns the `:59833` listener? (F-03 sink A, audit 2026-07-31-9c4cb0c.)
//!
//! A second instance that loses the bind asks `/health` "are you the accelerator?" and, on Windows,
//! exits when the answer is yes. Both fields it checks (`status=="ok"`, `api_version==1`) are public
//! contract, so ANY local process can answer them and make the genuine app quit — which also takes
//! down its HTTPS listener, and that is what opens F-01's plaintext window on Windows.
//!
//! **Why not a shared secret or a named mutex.** The modelled actor is a same-user local process. It
//! can read any `0600` file we own, so no file-based token authenticates anything; and it can create
//! a named mutex FIRST, so our failure to acquire is indistinguishable from a genuine sibling holding
//! it. Only an OS-mediated answer has the right asymmetry: ask the kernel which process owns the
//! socket, then compare that process's image path to our own.

/// Who owns the port, as far as the OS will tell us.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortOwner {
    /// The listener belongs to a process running our own image.
    Ours,
    /// The listener belongs to a process running a DIFFERENT image.
    Foreign,
    /// The OS would not say — no matching row, several rows, an unreadable image path, or a
    /// platform where we do not look.
    Unknown,
}

/// Classify a resolved owner image against our own. Pure, so the decision is table-tested on every
/// platform even though only Windows can produce the input.
///
/// Comparison is case-insensitive: Windows paths are case-insensitive, and the same executable can
/// legitimately be reported with different casing than `current_exe()` returns.
pub fn classify(owner_image: Option<&std::path::Path>, our_image: &std::path::Path) -> PortOwner {
    let Some(owner) = owner_image else {
        return PortOwner::Unknown;
    };
    // Canonicalize where possible so a short (8.3) path, a symlink, or a `..` segment does not read
    // as a different binary. Falls back to the raw path when the file is gone or unreadable.
    let owner_c = owner.canonicalize().unwrap_or_else(|_| owner.to_path_buf());
    let ours_c = our_image
        .canonicalize()
        .unwrap_or_else(|_| our_image.to_path_buf());
    if owner_c.as_os_str().eq_ignore_ascii_case(ours_c.as_os_str()) {
        PortOwner::Ours
    } else {
        PortOwner::Foreign
    }
}

/// May a redundant instance bow out (exit 0) after losing the bind? (**D-ITEM7** — the polarity here
/// is load-bearing and was an explicit owner decision.)
///
/// | verdict | action | vs. today |
/// |---|---|---|
/// | `Ours` | exit | identical |
/// | `Unknown` | **exit** | identical |
/// | `Foreign` | stay resident, surface the error | the fix |
///
/// The check may only ever **add** a reason to stay resident, never remove one — so this is exactly
/// behaviour-preserving on every path except the attack it closes.
///
/// **Why `Unknown` exits rather than staying.** The bow-out is a per-minute hot path on Windows, not
/// a once-per-logon edge: `crash_recovery.rs` registers a repeating `PT1M` trigger, and `main.rs`
/// notes "the Run-key-vs-tick race is absorbed by the exit-0-if-healthy guard". Treating `Unknown` as
/// "stay resident" would give a transient lookup failure ~1440 chances a day to strand a duplicate
/// tray process showing an error — a defect generator, not a residual risk.
///
/// The security cost is small and stated: an attacker who can force the lookup to fail keeps the
/// attack. But they are *holding the listening socket*, so their row is in the table; forcing
/// `Unknown` means releasing the port, which forfeits the squat.
pub fn may_bow_out(healthy_aztec_answered: bool, owner: PortOwner) -> bool {
    healthy_aztec_answered && owner != PortOwner::Foreign
}

/// Resolve the image path of the process owning the IPv4 listener on `port`, via
/// `GetExtendedTcpTable` + `QueryFullProcessImageNameW`.
///
/// `None` — i.e. [`PortOwner::Unknown`] — on any doubt: no row, MORE than one row (an ambiguous
/// answer must never be guessed into a verdict), a process we cannot open, or an unreadable path.
#[cfg(windows)]
pub fn owner_image_of_port(port: u16) -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INSUFFICIENT_BUFFER};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID,
        TCP_TABLE_OWNER_PID_LISTENER,
    };
    use windows_sys::Win32::Networking::WinSock::AF_INET;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // The table is racy by nature (rows come and go), so size then fetch, and tolerate one retry.
    let mut size: u32 = 0;
    let mut buf: Vec<u8> = Vec::new();
    for _ in 0..3 {
        let rc = unsafe {
            GetExtendedTcpTable(
                if buf.is_empty() {
                    std::ptr::null_mut()
                } else {
                    buf.as_mut_ptr().cast()
                },
                &mut size,
                0, // no sort — we filter ourselves
                AF_INET as u32,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if rc == 0 {
            break;
        }
        if rc != ERROR_INSUFFICIENT_BUFFER {
            return None;
        }
        buf = vec![0u8; size as usize];
    }
    if buf.is_empty() || (buf.len() as u32) < size {
        return None;
    }

    // SAFETY: on success the buffer holds a MIB_TCPTABLE_OWNER_PID whose `dwNumEntries` is followed
    // by that many MIB_TCPROW_OWNER_PID. Bounds are re-checked against the buffer length below
    // rather than trusted, so a malformed count cannot walk off the end.
    let table = unsafe { &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID) };
    let count = table.dwNumEntries as usize;
    let header =
        std::mem::size_of::<MIB_TCPTABLE_OWNER_PID>() - std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
    let row_size = std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
    if header + count.saturating_mul(row_size) > buf.len() {
        return None;
    }

    let want = port.to_be() as u32; // dwLocalPort is in network byte order, in the low 16 bits
    let mut pid: Option<u32> = None;
    for i in 0..count {
        let row =
            unsafe { &*(buf.as_ptr().add(header + i * row_size) as *const MIB_TCPROW_OWNER_PID) };
        if row.dwLocalPort != want {
            continue;
        }
        match pid {
            // Two listeners claiming the same port is exactly the ambiguity this must not resolve.
            Some(seen) if seen != row.dwOwningPid => return None,
            Some(_) => {}
            None => pid = Some(row.dwOwningPid),
        }
    }
    let pid = pid?;

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let mut wide = [0u16; 32768]; // MAX_PATH is not the real limit for this API
    let mut len = wide.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, wide.as_mut_ptr(), &mut len) };
    unsafe { CloseHandle(handle) };
    if ok == 0 || len == 0 {
        return None;
    }
    Some(std::path::PathBuf::from(std::ffi::OsString::from_wide(
        &wide[..len as usize],
    )))
}

/// Who owns [`super::PORT`]? Always [`PortOwner::Unknown`] off Windows, where the bow-out this
/// guards does not exist.
pub fn classify_port_owner(port: u16) -> PortOwner {
    #[cfg(windows)]
    {
        let Ok(ours) = std::env::current_exe() else {
            return PortOwner::Unknown;
        };
        return classify(owner_image_of_port(port).as_deref(), &ours);
    }
    #[cfg(not(windows))]
    {
        let _ = port;
        PortOwner::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn classify_table() {
        let ours = PathBuf::from("/opt/aztec/AztecAccelerator");
        assert_eq!(classify(Some(&ours), &ours), PortOwner::Ours);
        assert_eq!(
            classify(Some(Path::new("/tmp/evil/AztecAccelerator")), &ours),
            PortOwner::Foreign,
            "the same FILE NAME in another directory is a different binary"
        );
        assert_eq!(
            classify(None, &ours),
            PortOwner::Unknown,
            "no answer from the OS must not be guessed into a verdict"
        );
    }

    #[test]
    fn classify_is_case_insensitive() {
        assert_eq!(
            classify(
                Some(Path::new("C:\\Program Files\\Aztec\\AZTECACCELERATOR.EXE")),
                Path::new("C:\\Program Files\\Aztec\\AztecAccelerator.exe"),
            ),
            PortOwner::Ours,
        );
    }

    /// D-ITEM7. The whole point of the polarity: only a POSITIVE `Foreign` keeps us resident.
    #[test]
    fn bow_out_polarity_only_adds_reasons_to_stay() {
        assert!(may_bow_out(true, PortOwner::Ours), "unchanged from today");
        assert!(
            may_bow_out(true, PortOwner::Unknown),
            "Unknown must behave exactly as today — the bow-out runs every minute on Windows, so \
             treating a transient lookup failure as 'stay resident' would strand duplicate trays"
        );
        assert!(
            !may_bow_out(true, PortOwner::Foreign),
            "the fix: a squatter must not be able to evict the real app"
        );
        // And the pre-existing condition still gates everything.
        assert!(!may_bow_out(false, PortOwner::Ours));
        assert!(!may_bow_out(false, PortOwner::Unknown));
        assert!(!may_bow_out(false, PortOwner::Foreign));
    }

    /// Off Windows this must be inert — the bow-out it guards is Windows-only.
    #[cfg(not(windows))]
    #[test]
    fn non_windows_never_claims_to_know_the_owner() {
        assert_eq!(classify_port_owner(59833), PortOwner::Unknown);
    }

    /// The spike that decides whether item 7 ships at all: can we resolve a same-user socket's owner
    /// without elevation on a CI runner? Runs on the `windows-build` leg, which executes
    /// `cargo test` for this crate on `windows-latest`.
    #[cfg(windows)]
    #[test]
    fn resolves_the_owner_of_a_listener_this_process_opened() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let image = owner_image_of_port(port)
            .expect("GetExtendedTcpTable must resolve a same-user listener we opened ourselves");
        let ours = std::env::current_exe().unwrap();
        assert_eq!(
            classify(Some(&image), &ours),
            PortOwner::Ours,
            "a socket opened by THIS process must classify as Ours (got {image:?} vs {ours:?})"
        );
    }
}
