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
/// **The residual, stated accurately — this comment has been wrong twice, so it is worth reading.**
///
/// It first claimed that forcing `Unknown` requires releasing the socket, so an attacker could not
/// both squat and hide. Wrong: `OpenProcess` access is DACL-controlled, so a same-user squatter can
/// deny query access to itself while keeping the port. That case is caught now — it maps to
/// [`PortOwner::Foreign`] via `Lookup::Guarded`.
///
/// It then claimed the remaining routes to `Unknown` were "not attacker-controlled". Also wrong,
/// under the listener-scan design that preceded this one: a process could accept the health
/// connection, close only its LISTENING socket, and answer over the accepted socket, leaving the
/// scan with nobody to find. [`classify_port_owner`] no longer scans listeners for exactly this
/// reason.
///
/// What genuinely remains: an attacker who accepts our identification connection and tears it down
/// fast enough to win a race against the table lookup gets `Unknown`, and `Unknown` exits. It is a
/// race they must win on every probe rather than a state they can hold, and losing it means being
/// seen as `Foreign` — and if they drop the connection instead, the separate `/health` probe fails
/// too, so `healthy` is false and we stay resident anyway. Closing it entirely would mean treating
/// every transient failure as hostile, on a path that runs once a minute.
pub fn may_bow_out(healthy_aztec_answered: bool, owner: PortOwner) -> bool {
    healthy_aztec_answered && owner != PortOwner::Foreign
}

/// Probe `/health` AND identify the owner over **one** connection (F-03 sink A).
///
/// **Why one connection is not an optimisation.** Doing them separately is a deterministic bypass,
/// not a race: `main.rs` probes health first, so a one-shot listener can accept that request, close
/// its listening socket, and answer healthy over the accepted socket. A second connection then finds
/// nothing, yields `Unknown`, and `may_bow_out(true, Unknown)` exits — restoring the whole eviction
/// hole. Two connections can be answered by two different processes; only one connection ties
/// "answered healthy" to "is our image".
///
/// **Why a connection rather than the listener table.** Six review rounds each widened the "which
/// listener rows count as owning this port" predicate — port-only, loopback, the `0.0.0.0` wildcard,
/// the IPv6 table for dual-stack `::`, scan-failure vs. empty — and each widening exposed an adjacent
/// case, because "who is listening in a way that covers this address" is hard to enumerate. The OS has
/// already resolved all of it by the time it accepts, so the peer of a live connection is a single
/// unambiguous row.
///
/// Returns `(healthy, owner)`. Blocking, and Windows-only: the bow-out it serves is Windows-only.
#[cfg(windows)]
pub fn probe_and_identify(port: u16) -> (bool, PortOwner) {
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, SocketAddr, TcpStream};
    use std::time::Duration;

    const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
    const IO_TIMEOUT: Duration = Duration::from_secs(3);
    /// The real body is a few hundred bytes; this bounds a peer that answers `200` and streams.
    const MAX_BODY: usize = 64 * 1024;

    let target = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&target, CONNECT_TIMEOUT) else {
        return (false, PortOwner::Unknown);
    };
    let (Ok(local), Ok(peer)) = (stream.local_addr(), stream.peer_addr()) else {
        return (false, PortOwner::Unknown);
    };
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));

    // Identify FIRST, while the connection is certainly live — before any request can prompt the peer
    // to tear it down.
    let owner = match connection_peer_pid(peer, local) {
        Some(pid) => match image_of_pid(pid) {
            Lookup::Image(image) => match std::env::current_exe() {
                Ok(ours) => classify(Some(image.as_path()), &ours),
                Err(_) => PortOwner::Unknown,
            },
            Lookup::Guarded => PortOwner::Foreign,
            Lookup::NoAnswer => PortOwner::Unknown,
        },
        None => PortOwner::Unknown,
    };

    // Then ask the same peer, on the same stream, whether it is a healthy Aztec accelerator.
    let request = format!(
        "GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return (false, owner);
    }
    let mut raw = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if raw.len() + n > MAX_BODY {
                    return (false, owner); // over-cap: refuse to treat it as healthy
                }
                raw.extend_from_slice(&chunk[..n]);
            }
            Err(_) => break,
        }
    }
    (health_response_is_healthy(&raw), owner)
}

/// Is this raw HTTP/1.1 response a `2xx` carrying a healthy-Aztec `/health` body?
///
/// Pure, so the parse is table-tested on every platform even though only Windows produces the input.
#[cfg(any(windows, test))]
pub(crate) fn health_response_is_healthy(raw: &[u8]) -> bool {
    let Some(split) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
        return false;
    };
    let (head, body) = raw.split_at(split);
    let Ok(head) = std::str::from_utf8(head) else {
        return false;
    };
    let Some(status) = head.lines().next() else {
        return false;
    };
    // "HTTP/1.1 200 OK" — accept any 2xx, reject everything else, exactly as the reqwest probe does.
    let is_2xx = status
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse::<u16>().ok())
        .is_some_and(|c| (200..300).contains(&c));
    if !is_2xx {
        return false;
    }
    let body = &body[4.min(body.len())..];
    serde_json::from_slice::<serde_json::Value>(body)
        .map(|v| super::probe::is_healthy_aztec_response(&v))
        .unwrap_or(false)
}

/// The PID owning the connection whose local endpoint is `local` (the peer's side — i.e. our
/// `peer_addr`) and whose remote endpoint is `remote` (our own `local_addr`).
///
/// Matches the **complete four-tuple**, not just the port pair. Port 59833 lies inside Windows's
/// default ephemeral range and the same port may be bound on different explicit interfaces, so an
/// unrelated connection can share the reversed port pair and win by table order (post-impl codex
/// round 7). Duplicate exact matches are rejected rather than resolved by order.
#[cfg(windows)]
fn connection_peer_pid(local: std::net::SocketAddr, remote: std::net::SocketAddr) -> Option<u32> {
    use std::net::SocketAddr;
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
        MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_CONNECTIONS,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6};

    /// IPv4 address as the network-order `u32` the table stores.
    fn v4(addr: &SocketAddr) -> Option<u32> {
        match addr {
            SocketAddr::V4(a) => Some(u32::from_ne_bytes(a.ip().octets())),
            SocketAddr::V6(_) => None,
        }
    }
    /// The 16 address bytes as the table stores them, mapping IPv4 to `::ffff:a.b.c.d` — which is how
    /// a dual-stack listener's accepted connection appears in the TCP6 table.
    fn v6(addr: &SocketAddr) -> [u8; 16] {
        match addr {
            SocketAddr::V6(a) => a.ip().octets(),
            SocketAddr::V4(a) => a.ip().to_ipv6_mapped().octets(),
        }
    }

    let want_local_port = local.port().to_be() as u32;
    let want_remote_port = remote.port().to_be() as u32;
    let mut found: Option<u32> = None;

    // IPv4 table.
    if let (Some(want_local_addr), Some(want_remote_addr)) = (v4(&local), v4(&remote)) {
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
                let table =
                    unsafe { std::ptr::read_unaligned(bytes.cast::<MIB_TCPTABLE_OWNER_PID>()) };
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
                        if row.dwLocalPort == want_local_port
                            && row.dwRemotePort == want_remote_port
                            && row.dwLocalAddr == want_local_addr
                            && row.dwRemoteAddr == want_remote_addr
                        {
                            match found {
                                // Two rows for one exact four-tuple is an answer we must not pick from.
                                Some(seen) if seen != row.dwOwningPid => return None,
                                _ => found = Some(row.dwOwningPid),
                            }
                        }
                    }
                }
            }
        }
    }
    if found.is_some() {
        return found;
    }

    // IPv6 table — where a dual-stack listener's accepted connection lands, IPv4-mapped.
    let (want_local_addr, want_remote_addr) = (v6(&local), v6(&remote));
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
        if row.dwLocalPort == want_local_port
            && row.dwRemotePort == want_remote_port
            && row.ucLocalAddr == want_local_addr
            && row.ucRemoteAddr == want_remote_addr
        {
            match found {
                Some(seen) if seen != row.dwOwningPid => return None,
                _ => found = Some(row.dwOwningPid),
            }
        }
    }
    found
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

/// Off Windows there is no bow-out to guard, so nobody is ever identified and health is left to the
/// existing async probe. Present so the caller compiles everywhere without a `cfg` of its own.
///
/// It still goes through [`classify`] with no owner rather than returning `Unknown` directly: that
/// keeps the one piece of pure decision logic compiled, reachable and exercised on the platform where
/// the whole test suite actually runs, instead of dead code off Windows.
#[cfg(not(windows))]
pub fn probe_and_identify(_port: u16) -> (bool, PortOwner) {
    let ours = std::env::current_exe().unwrap_or_default();
    (false, classify(None, &ours))
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

    /// The raw-HTTP `/health` parse. Pure, so it is table-tested on every platform even though only
    /// Windows produces the input — and it exists at all because health and identity must come from
    /// ONE connection, which rules out the async client used elsewhere.
    #[test]
    fn health_response_parsing() {
        let ok = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\r\n{\"status\":\"ok\",\"api_version\":1}";
        assert!(health_response_is_healthy(ok));

        // Any 2xx, since that is what the async probe accepts.
        let created = b"HTTP/1.1 204 No Content\r\n\r\n{\"status\":\"ok\",\"api_version\":1}";
        assert!(health_response_is_healthy(created));

        // Non-2xx must never be trusted, however convincing the body.
        let not_found = b"HTTP/1.1 404 Not Found\r\n\r\n{\"status\":\"ok\",\"api_version\":1}";
        assert!(!health_response_is_healthy(not_found));

        // A 200 from something that is not our contract.
        let foreign = b"HTTP/1.1 200 OK\r\n\r\n{\"hello\":\"not the accelerator\"}";
        assert!(!health_response_is_healthy(foreign));

        // Wrong api_version is the case the shape check exists for.
        let wrong_version = b"HTTP/1.1 200 OK\r\n\r\n{\"status\":\"ok\",\"api_version\":2}";
        assert!(!health_response_is_healthy(wrong_version));

        // Truncated / headerless / empty inputs must not panic or pass.
        assert!(!health_response_is_healthy(b"HTTP/1.1 200 OK"));
        assert!(!health_response_is_healthy(b""));
        assert!(!health_response_is_healthy(b"\r\n\r\n"));
        assert!(!health_response_is_healthy(
            b"HTTP/1.1 200 OK\r\n\r\nnot json"
        ));
        // A non-UTF-8 header block must be rejected rather than panicking.
        assert!(!health_response_is_healthy(&[
            0xff, 0xfe, b'\r', b'\n', b'\r', b'\n'
        ]));
    }

    /// Off Windows this must be inert — the bow-out it guards is Windows-only.
    #[cfg(not(windows))]
    #[test]
    fn non_windows_never_claims_to_know_the_owner() {
        assert_eq!(probe_and_identify(59833), (false, PortOwner::Unknown));
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
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // Accept and answer a healthy /health, HOLDING the accepted socket for the lifetime of the
        // handler — dropping it immediately would tear the connection down mid-lookup.
        let accepter = std::thread::spawn(move || {
            if let Ok((mut sock, _)) = listener.accept() {
                let body = br#"{"status":"ok","api_version":1}"#;
                let head = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(head.as_bytes());
                let _ = sock.write_all(body);
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
        });
        let (healthy, owner) = probe_and_identify(port);
        assert!(healthy, "the peer answered a healthy /health body");
        assert_eq!(
            owner,
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

        let (_healthy, verdict) = probe_and_identify(port);

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
