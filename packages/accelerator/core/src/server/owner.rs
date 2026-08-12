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
/// **The residual, stated accurately.** An earlier version of this comment claimed that forcing
/// `Unknown` requires releasing the socket, so an attacker could not both squat and hide. **That was
/// wrong** (post-impl codex): `OpenProcess` access is DACL-controlled, so a same-user squatter can
/// deny query access to itself while keeping the port. That specific case is now caught — it maps to
/// [`PortOwner::Foreign`] via `Lookup::Guarded`, see [`classify_port_owner`] — but the honest residual
/// is that any OTHER route to `Unknown` (table churn, a PID that exits between the two calls) still
/// permits the bow-out. Those are transient and not attacker-controlled on demand; closing them would
/// mean treating every transient failure as hostile, on a path that runs once a minute.
pub fn may_bow_out(healthy_aztec_answered: bool, owner: PortOwner) -> bool {
    healthy_aztec_answered && owner != PortOwner::Foreign
}

/// Resolve who owns `port`, by CONNECTING to it and asking the OS who owns that connection.
///
/// **Why a connection rather than the listener table.** Six successive review rounds each widened the
/// "which listener rows count as owning this port" predicate — port-only, then loopback, then the
/// `0.0.0.0` wildcard, then the IPv6 table for dual-stack `::`, then scan-failure vs. empty. Each
/// widening was correct and each exposed an adjacent case, because "who is listening in a way that
/// covers this address" is genuinely hard to enumerate: wildcards, address families, `IPV6_V6ONLY`,
/// and several coexisting listeners all bear on it.
///
/// A connection sidesteps every one of them. The OS has already done that routing by the time it
/// accepts our connect, so the answer is a single row identified by an **exact port pair** — our
/// ephemeral local port is unique, so no address predicate is needed at all. There is one owner, so
/// there is nothing to aggregate.
///
/// It also closes a real bypass the listener scan had: a process could accept our health connection,
/// close only its LISTENING socket, and answer over the accepted socket — after which the listener
/// scan found nobody, returned `Unknown`, and the genuine app bowed out. A connection row survives
/// for as long as the connection does, so closing the listener no longer hides the owner.
#[cfg(windows)]
fn owner_image_of_port(port: u16) -> Lookup {
    use std::net::{Ipv4Addr, SocketAddr, TcpStream};
    use std::time::Duration;

    // A short timeout: this is loopback, and the caller is on the startup path.
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else {
        // Nothing accepted — nobody owns it in any sense we care about.
        return Lookup::NoAnswer;
    };
    let Ok(local) = stream.local_addr() else {
        return Lookup::NoAnswer;
    };
    let ephemeral = local.port();

    // From the OS's side the peer's row is (local = our target port, remote = our ephemeral port).
    // The pair is unique while the connection is open, which is why no address matching is needed —
    // and the connection is held open across the lookup precisely so the row cannot vanish.
    let pid = connection_peer_pid(port, ephemeral);
    drop(stream);
    match pid {
        Some(pid) => image_of_pid(pid),
        None => Lookup::NoAnswer,
    }
}

/// The PID owning the connection whose local port is `local_port` and whose remote port is
/// `remote_port`, searching the IPv4 connection table and then the IPv6 one.
///
/// Matching is on the PORT PAIR alone. An ephemeral port is unique among live connections to this
/// service, so addresses add nothing — and it is exactly the address reasoning that this design
/// exists to delete. The v6 table is still consulted because a dual-stack listener's accepted
/// connection appears there as IPv4-mapped.
#[cfg(windows)]
fn connection_peer_pid(local_port: u16, remote_port: u16) -> Option<u32> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
        MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_CONNECTIONS,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};

    let want_local = local_port.to_be() as u32;
    let want_remote = remote_port.to_be() as u32;

    if let Some((words, capacity)) =
        fetch_tcp_table(AF_INET as u32, TCP_TABLE_OWNER_PID_CONNECTIONS)
    {
        let bytes = words.as_ptr().cast::<u8>();
        let header = std::mem::size_of::<MIB_TCPTABLE_OWNER_PID>()
            - std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
        let row_size = std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
        if capacity >= std::mem::size_of::<MIB_TCPTABLE_OWNER_PID>() {
            // SAFETY: `read_unaligned` imposes no alignment requirement, and the row count is
            // bounds-checked against `capacity` before any row is read.
            let table = unsafe { std::ptr::read_unaligned(bytes.cast::<MIB_TCPTABLE_OWNER_PID>()) };
            let count = table.dwNumEntries as usize;
            if header + count.saturating_mul(row_size) <= capacity {
                for i in 0..count {
                    let row = unsafe {
                        std::ptr::read_unaligned(
                            bytes
                                .add(header + i * row_size)
                                .cast::<MIB_TCPROW_OWNER_PID>(),
                        )
                    };
                    if row.dwLocalPort == want_local && row.dwRemotePort == want_remote {
                        return Some(row.dwOwningPid);
                    }
                }
            }
        }
    }

    let (words, capacity) = fetch_tcp_table(AF_INET6 as u32, TCP_TABLE_OWNER_PID_CONNECTIONS)?;
    let bytes = words.as_ptr().cast::<u8>();
    let header = std::mem::size_of::<MIB_TCP6TABLE_OWNER_PID>()
        - std::mem::size_of::<MIB_TCP6ROW_OWNER_PID>();
    let row_size = std::mem::size_of::<MIB_TCP6ROW_OWNER_PID>();
    if capacity < std::mem::size_of::<MIB_TCP6TABLE_OWNER_PID>() {
        return None;
    }
    // SAFETY: as above.
    let table = unsafe { std::ptr::read_unaligned(bytes.cast::<MIB_TCP6TABLE_OWNER_PID>()) };
    let count = table.dwNumEntries as usize;
    if header + count.saturating_mul(row_size) > capacity {
        return None;
    }
    for i in 0..count {
        let row = unsafe {
            std::ptr::read_unaligned(
                bytes
                    .add(header + i * row_size)
                    .cast::<MIB_TCP6ROW_OWNER_PID>(),
            )
        };
        if row.dwLocalPort == want_local && row.dwRemotePort == want_remote {
            return Some(row.dwOwningPid);
        }
    }
    None
}

/// Fetch an extended TCP table for `family` and `class`, as `u32` words plus the valid byte length.
///
/// The storage is over-aligned deliberately: `Vec<u8>` has alignment 1, and dereferencing it as a
/// 4-aligned table struct is undefined behaviour even where x86 tolerates it. Rows are still read
/// with `read_unaligned`, because the table may carry padding between entries.
#[cfg(windows)]
fn fetch_tcp_table(family: u32, class: i32) -> Option<(Vec<u32>, usize)> {
    use windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER;
    use windows_sys::Win32::NetworkManagement::IpHelper::GetExtendedTcpTable;

    let mut words: Vec<u32> = Vec::new();
    let mut size: u32 = 0;
    for _ in 0..4 {
        let ptr = if words.is_empty() {
            std::ptr::null_mut()
        } else {
            words.as_mut_ptr().cast()
        };
        let rc = unsafe { GetExtendedTcpTable(ptr, &mut size, 0, family, class, 0) };
        if rc == 0 {
            let capacity = (words.len() * 4).min(size as usize);
            return if words.is_empty() {
                None
            } else {
                Some((words, capacity))
            };
        }
        if rc != ERROR_INSUFFICIENT_BUFFER {
            return None;
        }
        // The table can grow between the sizing call and the fetch, hence the loop rather than one
        // resize — but never return having only RESIZED, which would read an unfilled buffer.
        words = vec![0u32; (size as usize).div_ceil(4)];
    }
    None
}

/// Resolve one PID's image, distinguishing "refused" from "gone".
#[cfg(windows)]
fn image_of_pid(pid: u32) -> Lookup {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        // Distinguish "it is gone" from "it refused to be identified". `OpenProcess` access is
        // DACL-controlled, so a same-user squatter can deny `PROCESS_QUERY_LIMITED_INFORMATION` on
        // ITSELF while keeping the socket — which would otherwise force `Unknown` and hand it the
        // eviction anyway (post-impl codex). A genuine sibling of ours sets no such DACL.
        return if unsafe { GetLastError() } == ERROR_ACCESS_DENIED {
            Lookup::Guarded
        } else {
            Lookup::NoAnswer
        };
    }
    let mut wide = [0u16; 32768]; // MAX_PATH is not the real limit for this API
    let mut len = wide.len() as u32;
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, wide.as_mut_ptr(), &mut len) };
    unsafe { CloseHandle(handle) };
    if ok == 0 || len == 0 {
        return Lookup::NoAnswer;
    }
    Lookup::Image(std::path::PathBuf::from(std::ffi::OsString::from_wide(
        &wide[..len as usize],
    )))
}

/// What the OS lookup came back with, before it is judged against our own image.
///
/// Only the Windows lookup constructs `Image` and `Guarded` — off Windows the stub below always
/// answers `NoAnswer`, so those two are legitimately never built there. The `match` in
/// [`classify_port_owner`] still handles all three on every platform, which is the point: the
/// decision is shared, only the lookup is platform-specific.
#[cfg_attr(not(windows), allow(dead_code))]
enum Lookup {
    /// The owning process's image path.
    Image(std::path::PathBuf),
    /// No usable answer: no matching row, a process that had already exited, or a platform where we
    /// do not look. Benign and usually transient.
    NoAnswer,
    /// A process owns the socket but **refused to be identified** — `OpenProcess` returned
    /// `ERROR_ACCESS_DENIED`. Not transient, and not something a sibling of ours does.
    Guarded,
}

/// Off Windows we do not look, so nobody is ever identified.
///
/// A stub rather than a `cfg` branch inside [`classify_port_owner`] so the DECISION path is identical
/// on every platform: only the *lookup* is platform-specific, and `classify` is therefore compiled,
/// reachable and exercised everywhere rather than being dead code off Windows.
#[cfg(not(windows))]
fn owner_image_of_port(_port: u16) -> Lookup {
    Lookup::NoAnswer
}

/// Who owns [`super::PORT`]?
///
/// `Guarded` maps to [`PortOwner::Foreign`], which is the one place this deviates from "an
/// unidentified owner is Unknown". `OpenProcess` access is DACL-controlled, so a same-user squatter
/// can deny query access to ITSELF while holding the socket; treating that as Unknown would let it
/// force the bow-out and keep the whole attack (post-impl codex). Refusing to be identified is not
/// something our own build does, so it is judged as what it is.
pub fn classify_port_owner(port: u16) -> PortOwner {
    let Ok(ours) = std::env::current_exe() else {
        return PortOwner::Unknown;
    };
    match owner_image_of_port(port) {
        Lookup::Image(image) => classify(Some(image.as_path()), &ours),
        Lookup::Guarded => PortOwner::Foreign,
        Lookup::NoAnswer => PortOwner::Unknown,
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

    /// Can we resolve a same-user socket's owner without elevation on a CI runner? This is the
    /// question item 7 ships or defers on, and it runs on the `windows-build` leg, which executes
    /// `cargo test` for this crate on `windows-latest`.
    ///
    /// A listener we opened ourselves must classify as `Ours` — which also exercises the connect,
    /// the connection-table scan and the image comparison end to end.
    #[cfg(windows)]
    #[test]
    fn a_listener_this_process_opened_classifies_as_ours() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // Accept in the background so our probe connection completes.
        let accepter = std::thread::spawn(move || {
            let _ = listener.accept();
            // Hold the accepted socket briefly so the connection row is live during the lookup.
            std::thread::sleep(std::time::Duration::from_millis(300));
        });
        assert_eq!(
            classify_port_owner(port),
            PortOwner::Ours,
            "a socket opened by THIS process must classify as Ours"
        );
        let _ = accepter.join();
    }

    /// **The two-instance test.** A listener owned by a DIFFERENT executable must classify as
    /// `Foreign`, which is the only verdict that keeps a redundant instance resident — the entire
    /// point of F-03 sink A. A design argument is not evidence for this; a second real process is.
    ///
    /// Uses a stock Windows binary as the foreign owner, so the test needs no fixture of its own.
    #[cfg(windows)]
    #[test]
    fn a_listener_owned_by_another_executable_classifies_as_foreign() {
        use std::io::{BufRead, BufReader};
        use std::process::{Command, Stdio};

        // powershell.exe is a different image from the test binary, and can hold a listener and
        // accept one connection — exactly the shape of the squatter this guard exists to catch.
        let script = concat!(
            "$l=[System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback,0);",
            "$l.Start();Write-Output $l.LocalEndpoint.Port;[Console]::Out.Flush();",
            "$c=$l.AcceptTcpClient();Start-Sleep -Milliseconds 1500;$c.Close();$l.Stop()"
        );
        let mut child = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .stdout(Stdio::piped())
            .spawn()
            .expect("could not spawn a second process to own a port");

        let stdout = child.stdout.take().expect("child stdout");
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("child must report its port");
        let port: u16 = line.trim().parse().expect("child must print a port number");

        let verdict = classify_port_owner(port);

        let _ = child.kill();
        let _ = child.wait();

        assert_eq!(
            verdict,
            PortOwner::Foreign,
            "a port held by another executable must be Foreign — this is the verdict that stops a \
             squatter evicting the real app"
        );
        // And the decision that verdict drives: never bow out to it.
        assert!(!may_bow_out(true, verdict));
    }
}
