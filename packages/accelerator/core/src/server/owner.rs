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

/// Resolve the image path of the process owning the IPv4 listener on `port`, via
/// `GetExtendedTcpTable` + `QueryFullProcessImageNameW`.
///
/// `None` — i.e. [`PortOwner::Unknown`] — on any doubt: no row, MORE than one row (an ambiguous
/// answer must never be guessed into a verdict), a process we cannot open, or an unreadable path.
#[cfg(windows)]
fn owner_image_of_port(port: u16) -> Lookup {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER,
    };
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID,
        TCP_TABLE_OWNER_PID_LISTENER,
    };
    use windows_sys::Win32::Networking::WinSock::AF_INET;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // Over-align the storage. `Vec<u8>` has alignment 1, and dereferencing it as a 4-aligned
    // `MIB_TCPTABLE_OWNER_PID` is undefined behaviour even where x86 tolerates it in practice
    // (post-impl codex). Backing it with `u32` gives the alignment the structs require; rows are
    // still read with `read_unaligned` because the table may carry padding between entries.
    let mut words: Vec<u32> = Vec::new();
    let mut size: u32 = 0;
    let mut fetched = false;
    for _ in 0..4 {
        let ptr = if words.is_empty() {
            std::ptr::null_mut()
        } else {
            words.as_mut_ptr().cast()
        };
        let rc = unsafe {
            GetExtendedTcpTable(
                ptr,
                &mut size,
                0, // unsorted — we filter ourselves
                AF_INET as u32,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if rc == 0 {
            fetched = true;
            break;
        }
        if rc != ERROR_INSUFFICIENT_BUFFER {
            return Lookup::NoAnswer;
        }
        // The table grows between the sizing call and the fetch, hence the loop rather than one
        // resize — but never leave it having only RESIZED, which would read an unfilled buffer.
        words = vec![0u32; (size as usize).div_ceil(4)];
    }
    if !fetched || words.is_empty() {
        return Lookup::NoAnswer;
    }

    let bytes = words.as_ptr().cast::<u8>();
    let capacity = (words.len() * 4).min(size as usize);
    // The row array begins where the single inline row does; deriving the header this way keeps it
    // correct if the struct ever gains fields.
    let header =
        std::mem::size_of::<MIB_TCPTABLE_OWNER_PID>() - std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
    let row_size = std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
    if capacity < std::mem::size_of::<MIB_TCPTABLE_OWNER_PID>() {
        return Lookup::NoAnswer;
    }
    // SAFETY: `bytes` points at `capacity` initialised bytes; `read_unaligned` imposes no alignment
    // requirement, and every row index is bounds-checked against `capacity` before it is read.
    let table = unsafe { std::ptr::read_unaligned(bytes.cast::<MIB_TCPTABLE_OWNER_PID>()) };
    let count = table.dwNumEntries as usize;
    if header + count.saturating_mul(row_size) > capacity {
        return Lookup::NoAnswer;
    }

    // Both fields are network byte order.
    //
    // Accept the WILDCARD address as well as loopback. A listener on `0.0.0.0:59833` accepts the
    // `127.0.0.1` connection our probe makes and blocks our specific bind just the same — filtering
    // to loopback alone (which is what round 1 of review asked for, and it was too narrow) binned
    // such an owner as `NoAnswer`, i.e. straight back to the bow-out this exists to guard.
    let want_port = port.to_be() as u32;
    let loopback = u32::from_ne_bytes([127, 0, 0, 1]);
    let wildcard = u32::from_ne_bytes([0, 0, 0, 0]);
    let mut pids: Vec<u32> = Vec::new();
    for i in 0..count {
        let row = unsafe {
            std::ptr::read_unaligned(
                bytes
                    .add(header + i * row_size)
                    .cast::<MIB_TCPROW_OWNER_PID>(),
            )
        };
        if row.dwLocalPort != want_port {
            continue;
        }
        if row.dwLocalAddr != loopback && row.dwLocalAddr != wildcard {
            continue;
        }
        if !pids.contains(&row.dwOwningPid) {
            pids.push(row.dwOwningPid);
        }
    }
    if pids.is_empty() {
        return Lookup::NoAnswer;
    }
    // Several owners CAN legitimately coexist on one port (a wildcard listener alongside a specific
    // one). Resolving every one of them and judging conservatively beats declaring the whole answer
    // ambiguous, which would also have landed on the bow-out.
    Lookup::Owners(pids.into_iter().map(image_of_pid).collect())
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
    /// One entry per distinct process owning the port. More than one is legitimate — a wildcard
    /// listener can coexist with a specific one — so every owner is resolved and judged rather than
    /// the whole answer being discarded as ambiguous.
    Owners(Vec<Lookup>),
}

/// Reduce per-owner verdicts to one, conservatively (F-03 sink A).
///
/// Pure, so the rule is table-tested on every platform even though only Windows produces the input:
/// - **any** owner that is foreign or refuses identification ⇒ `Foreign` (do not bow out);
/// - **all** owners identified as our own image ⇒ `Ours`;
/// - anything else ⇒ `Unknown`.
///
/// "Any foreign wins" is the direction that matters: with a squatter alongside a genuine sibling, the
/// squatter is the reason to stay resident.
pub fn aggregate(verdicts: &[PortOwner]) -> PortOwner {
    if verdicts.is_empty() {
        return PortOwner::Unknown;
    }
    if verdicts.contains(&PortOwner::Foreign) {
        return PortOwner::Foreign;
    }
    if verdicts.iter().all(|v| *v == PortOwner::Ours) {
        return PortOwner::Ours;
    }
    PortOwner::Unknown
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
    judge(owner_image_of_port(port), &ours)
}

/// Turn one lookup result into a verdict, recursing through the multi-owner case.
fn judge(lookup: Lookup, ours: &std::path::Path) -> PortOwner {
    match lookup {
        Lookup::Image(image) => classify(Some(image.as_path()), ours),
        Lookup::Guarded => PortOwner::Foreign,
        Lookup::NoAnswer => PortOwner::Unknown,
        Lookup::Owners(owners) => {
            let verdicts: Vec<PortOwner> = owners.into_iter().map(|o| judge(o, ours)).collect();
            aggregate(&verdicts)
        }
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

    /// A port can legitimately have several owners — a wildcard `0.0.0.0` listener alongside a
    /// specific `127.0.0.1` one. Round 1 of review narrowed the address filter to loopback only,
    /// which binned a wildcard owner as "no answer" and handed it the bow-out; round 2 caught that.
    /// The aggregate rule is what makes several owners a decision rather than a shrug.
    #[test]
    fn several_owners_are_judged_conservatively() {
        use PortOwner::{Foreign, Ours, Unknown};
        // One squatter alongside a genuine sibling is still a reason to stay resident.
        assert_eq!(aggregate(&[Ours, Foreign]), Foreign);
        assert_eq!(aggregate(&[Foreign, Ours]), Foreign);
        // A refused identification already maps to Foreign before it reaches here.
        assert_eq!(aggregate(&[Ours, Ours]), Ours);
        // Anything unresolved among otherwise-ours owners is not "ours".
        assert_eq!(aggregate(&[Ours, Unknown]), Unknown);
        assert_eq!(aggregate(&[Unknown]), Unknown);
        // No owners at all is not a verdict.
        assert_eq!(aggregate(&[]), Unknown);
    }

    /// The spike that decides whether item 7 ships at all: can we resolve a same-user socket's owner
    /// without elevation on a CI runner? Runs on the `windows-build` leg, which executes
    /// `cargo test` for this crate on `windows-latest`.
    #[cfg(windows)]
    #[test]
    fn resolves_the_owner_of_a_listener_this_process_opened() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let Lookup::Image(image) = owner_image_of_port(port) else {
            panic!("GetExtendedTcpTable must resolve a same-user listener we opened ourselves");
        };
        let ours = std::env::current_exe().unwrap();
        assert_eq!(
            classify(Some(&image), &ours),
            PortOwner::Ours,
            "a socket opened by THIS process must classify as Ours (got {image:?} vs {ours:?})"
        );
        // The whole path, including the loopback-address filter and the Guarded mapping.
        assert_eq!(classify_port_owner(port), PortOwner::Ours);
    }
}
