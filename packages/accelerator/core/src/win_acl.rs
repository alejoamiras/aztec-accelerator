//! F-003 Windows tail: owner-only ACLs on the private prove workspace (+ witness), the leaf TLS key, and
//! `config.json`. On Unix these paths are already `0o700`/`0o600` at creation; on Windows the mode bits are
//! a no-op, so without this a file inherits its parent's (potentially group-readable) ACL.
//!
//! Design (folds the dual-audit + codex-final FFI conditions):
//! - **Reparse-safe + existence-atomic**: objects are created with `CREATE_NEW` / `CreateDirectoryW`, which
//!   FAIL if anything already exists at the path — so a pre-planted symlink/junction can't be adopted.
//! - **PROTECTED DACL**: the ACL is applied to the OPEN HANDLE via `SetSecurityInfo` with
//!   `PROTECTED_DACL_SECURITY_INFORMATION`, which strips inherited parent ACEs (handle-based does NOT follow
//!   names, unlike `SetNamedSecurityInfoW`). The narrow window between create and apply carries only the
//!   default per-user `%LOCALAPPDATA%` ACL (owner+SYSTEM+Admins, never world).
//! - **Fail-closed readback**: after applying, the effective DACL is read back off the handle and asserted
//!   owner-only; a FAT/exFAT/network volume that silently no-ops ACL calls therefore returns an error rather
//!   than a falsely-"secured" path.
//! - **Memory hygiene**: the token handle is `CloseHandle`d; the `SetEntriesInAclW` ACL and every
//!   `GetSecurityInfo` security descriptor are `LocalFree`d exactly once on every path (RAII guards); the
//!   SID is copied out of the token buffer (never aliased/freed separately).
//!
//! Windows-gating lives ONLY at the module declaration (`lib.rs`: `#[cfg(windows)] pub mod win_acl;`)
//! — the authoritative gate. This file deliberately carries no inner `#![cfg(windows)]`: a duplicated
//! gate made clippy (x86_64-pc-windows-gnu) warn "duplicated attribute" and risk exactly the
//! detached-cfg confusion the repo has been bitten by before (IH-BUG-2).

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::Path;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_ALREADY_EXISTS, GENERIC_WRITE, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Security::Authorization::{
    GetSecurityInfo, SetEntriesInAclW, SetSecurityInfo, EXPLICIT_ACCESS_W, SET_ACCESS,
    SE_FILE_OBJECT, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
};
use windows_sys::Win32::Security::{
    CopySid, EqualSid, GetAce, GetLengthSid, GetTokenInformation, IsWellKnownSid, TokenUser,
    WinBuiltinUsersSid, WinWorldSid, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL,
    DACL_SECURITY_INFORMATION, NO_INHERITANCE, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSID, SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_QUERY,
    TOKEN_USER,
};

/// `ACCESS_ALLOWED_ACE_TYPE` (winnt.h `0x0`) — not re-exported by windows-sys under this path, so pinned
/// to its documented value. Only this ACE type has the `SidStart` layout `verify_owner_only` reads.
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    CREATE_NEW, FILE_ALL_ACCESS, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, OPEN_EXISTING, WRITE_OWNER,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// RAII: close a kernel handle exactly once.
struct HandleGuard(HANDLE);
impl Drop for HandleGuard {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe { CloseHandle(self.0) };
        }
    }
}

/// RAII: `LocalFree` a `LocalAlloc`-owned pointer (SetEntriesInAclW ACL, GetSecurityInfo descriptor).
struct LocalFreeGuard(*mut core::ffi::c_void);
impl Drop for LocalFreeGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0) };
        }
    }
}

fn last_err() -> io::Error {
    io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
}

/// Reject a handle that refers to a reparse point (junction/symlink). Closes the create→open TOCTOU on
/// `secure_create_dir` + `harden_existing`: a path swapped for a junction between create and open is opened
/// as the link itself (FILE_FLAG_OPEN_REPARSE_POINT), so its handle carries FILE_ATTRIBUTE_REPARSE_POINT.
unsafe fn reject_if_reparse(handle: HANDLE) -> io::Result<()> {
    let mut info: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
    if GetFileInformationByHandle(handle, &mut info) == 0 {
        return Err(last_err());
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "path is a reparse point (junction/symlink) — refusing to apply an owner-only ACL",
        ));
    }
    Ok(())
}

/// Wide-encode a path with a trailing NUL for the `*W` Win32 APIs.
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// The current process user's SID, copied into an owned buffer (so it outlives the token buffer it was
/// read from — the SID inside `TOKEN_USER` is a pointer INTO that buffer).
fn current_user_sid() -> io::Result<Vec<u8>> {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(last_err());
        }
        let _tguard = HandleGuard(token);

        // Two-call sizing.
        let mut len: u32 = 0;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
        if len == 0 {
            return Err(last_err());
        }
        let mut buf = vec![0u8; len as usize];
        if GetTokenInformation(token, TokenUser, buf.as_mut_ptr() as *mut _, len, &mut len) == 0 {
            return Err(last_err());
        }
        // `buf` is a `Vec<u8>` (align 1) but `TOKEN_USER` contains a pointer (align 8) — taking a
        // `&TOKEN_USER` reference to a possibly-misaligned address is UB. Read the `User.Sid` pointer field
        // via `addr_of!` + `read_unaligned` (no reference is ever formed). (codex post-impl #1)
        let tu = buf.as_ptr() as *const TOKEN_USER;
        let sid_ptr: PSID = std::ptr::addr_of!((*tu).User.Sid).read_unaligned();
        let sid_len = GetLengthSid(sid_ptr);
        let mut sid = vec![0u8; sid_len as usize];
        if CopySid(sid_len, sid.as_mut_ptr() as PSID, sid_ptr) == 0 {
            return Err(last_err());
        }
        Ok(sid)
    }
}

/// Apply an owner-only PROTECTED DACL (current user, full control) to an open handle, then read it back and
/// assert it took effect. `inheritable` adds container/object inheritance (for directories, so children are
/// private at creation).
unsafe fn apply_and_verify_owner_only(handle: HANDLE, inheritable: bool) -> io::Result<()> {
    // Close the create/open TOCTOU (codex post-impl #3): if the path was swapped for a junction between
    // creation and this open, the handle (opened FILE_FLAG_OPEN_REPARSE_POINT) is a reparse point — refuse
    // rather than secure the link while real accesses follow it to a non-owner-only target.
    reject_if_reparse(handle)?;
    let mut sid = current_user_sid()?;

    // One EXPLICIT_ACCESS: grant full control to the current user, inheritance per `inheritable`.
    let mut ea: EXPLICIT_ACCESS_W = std::mem::zeroed();
    ea.grfAccessPermissions = FILE_ALL_ACCESS;
    ea.grfAccessMode = SET_ACCESS;
    ea.grfInheritance = if inheritable {
        SUB_CONTAINERS_AND_OBJECTS_INHERIT
    } else {
        NO_INHERITANCE
    };
    ea.Trustee = std::mem::zeroed::<TRUSTEE_W>();
    ea.Trustee.TrusteeForm = TRUSTEE_IS_SID;
    ea.Trustee.TrusteeType = TRUSTEE_IS_USER;
    ea.Trustee.ptstrName = sid.as_mut_ptr() as *mut u16;

    let mut acl: *mut ACL = std::ptr::null_mut();
    let rc = SetEntriesInAclW(1, &ea, std::ptr::null_mut(), &mut acl);
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc as i32));
    }
    let _acl_guard = LocalFreeGuard(acl as *mut _);

    // PROTECTED_DACL strips inherited ACEs; handle-based SetSecurityInfo does not follow the name.
    // codex #6 / F-003: ALSO set the OWNER to the current user. The Windows owner ALWAYS holds implicit
    // READ_CONTROL + WRITE_DAC, so a foreign owner (e.g. Administrators as the default owner for an
    // elevated creator) could rewrite our owner-only DACL regardless of the ACEs — set + verify the owner
    // to close that bypass. Setting the owner to our own token SID needs WRITE_OWNER (requested at open).
    let rc = SetSecurityInfo(
        handle,
        SE_FILE_OBJECT,
        OWNER_SECURITY_INFORMATION
            | DACL_SECURITY_INFORMATION
            | PROTECTED_DACL_SECURITY_INFORMATION,
        sid.as_mut_ptr() as PSID,
        std::ptr::null_mut(),
        acl,
        std::ptr::null_mut(),
    );
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc as i32));
    }

    // Fail-closed readback: catches FAT/exFAT / network volumes that silently ignore ACLs, AND a foreign
    // owner that would retain WRITE_DAC over our DACL.
    verify_owner_only(handle, &sid)?;
    verify_owner_sid(handle, &sid)
}

/// Read the OWNER back off the handle and assert it is the current user. The Windows owner ALWAYS has
/// implicit `READ_CONTROL` + `WRITE_DAC`, so a non-owner-only DACL is meaningless if a foreign principal
/// owns the object — it can simply rewrite the DACL. Fail-closed if the owner is absent or foreign
/// (codex audit #6).
unsafe fn verify_owner_sid(handle: HANDLE, sid: &[u8]) -> io::Result<()> {
    let mut owner: PSID = std::ptr::null_mut();
    let mut sd: *mut core::ffi::c_void = std::ptr::null_mut();
    let rc = GetSecurityInfo(
        handle,
        SE_FILE_OBJECT,
        OWNER_SECURITY_INFORMATION,
        &mut owner,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        &mut sd,
    );
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc as i32));
    }
    let _sd_guard = LocalFreeGuard(sd);
    if owner.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "object has no owner after applying owner-only ACL",
        ));
    }
    if EqualSid(owner, sid.as_ptr() as PSID) == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "object owner is not the current user (foreign owner retains WRITE_DAC)",
        ));
    }
    Ok(())
}

/// Read the effective DACL back off the handle and assert it grants EXACTLY the given SID — no
/// `BUILTIN\Users`, no `Everyone`. Errors (fail-closed) if the DACL is absent or any ACE is foreign.
unsafe fn verify_owner_only(handle: HANDLE, sid: &[u8]) -> io::Result<()> {
    let mut dacl: *mut ACL = std::ptr::null_mut();
    let mut sd: *mut core::ffi::c_void = std::ptr::null_mut();
    let rc = GetSecurityInfo(
        handle,
        SE_FILE_OBJECT,
        DACL_SECURITY_INFORMATION,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        &mut dacl,
        std::ptr::null_mut(),
        &mut sd,
    );
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc as i32));
    }
    let _sd_guard = LocalFreeGuard(sd);
    if dacl.is_null() {
        // A null DACL means "everyone full access" — the FAT/exFAT no-op case. Fail closed.
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner-only ACL not applied (null DACL — unsupported filesystem?)",
        ));
    }
    let ace_count = (*dacl).AceCount as u32;
    if ace_count == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner-only ACL not applied (empty DACL)",
        ));
    }
    let want: PSID = sid.as_ptr() as PSID;
    for i in 0..ace_count {
        let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
        if GetAce(dacl, i, &mut ace) == 0 {
            return Err(last_err());
        }
        // Verify the ACE TYPE before casting: only ACCESS_ALLOWED_ACE has the SidStart layout we read.
        // Anything else (a DENY/AUDIT/OBJECT ACE we never set) means the DACL isn't the owner-only ACL we
        // applied → fail closed rather than misparse a foreign ACE's bytes as a SID.
        let header = &*(ace as *const ACE_HEADER);
        if header.AceType != ACCESS_ALLOWED_ACE_TYPE {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "unexpected ACE type after applying owner-only ACL",
            ));
        }
        let allowed = &*(ace as *const ACCESS_ALLOWED_ACE);
        let ace_sid = &allowed.SidStart as *const u32 as PSID;
        if EqualSid(ace_sid, want) == 0 {
            // Reject a foreign ACE — especially well-known world/users SIDs.
            if IsWellKnownSid(ace_sid, WinWorldSid) != 0
                || IsWellKnownSid(ace_sid, WinBuiltinUsersSid) != 0
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "world/users ACE present after applying owner-only ACL",
                ));
            }
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "foreign ACE present after applying owner-only ACL",
            ));
        }
        // Our ACE must grant FULL control — verify the access mask, not just the SID. (codex post-impl #2)
        if allowed.Mask & FILE_ALL_ACCESS != FILE_ALL_ACCESS {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "owner ACE does not grant full control after applying owner-only ACL",
            ));
        }
    }
    Ok(())
}

/// Create a directory with an owner-only PROTECTED, INHERITABLE DACL. Fails if the path already exists
/// (reparse/symlink pre-plant defense).
pub fn secure_create_dir(path: &Path) -> io::Result<()> {
    let w = wide(path);
    unsafe {
        if CreateDirectoryW(w.as_ptr(), std::ptr::null_mut()) == 0 {
            let e = GetLastError();
            // ERROR_ALREADY_EXISTS ⇒ something is already at the path; fail closed.
            return Err(io::Error::from_raw_os_error(e as i32));
        }
        // Open a handle to the just-created directory to apply the DACL + owner (WRITE_OWNER, codex #6).
        let handle = CreateFileW(
            w.as_ptr(),
            windows_sys::Win32::Storage::FileSystem::WRITE_DAC
                | windows_sys::Win32::Storage::FileSystem::READ_CONTROL
                | WRITE_OWNER,
            0,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return Err(last_err());
        }
        let _hg = HandleGuard(handle);
        apply_and_verify_owner_only(handle, true)
    }
}

/// Create a NEW file with an owner-only PROTECTED DACL and return it (ready to write). `CREATE_NEW` fails
/// if the path exists, so a pre-planted file/symlink is rejected; the ACL is applied to the empty file
/// BEFORE any content is written.
pub fn secure_create_file(path: &Path) -> io::Result<std::fs::File> {
    let w = wide(path);
    unsafe {
        let handle = CreateFileW(
            w.as_ptr(),
            GENERIC_WRITE
                | windows_sys::Win32::Storage::FileSystem::WRITE_DAC
                | windows_sys::Win32::Storage::FileSystem::READ_CONTROL
                | WRITE_OWNER,
            0, // no sharing
            std::ptr::null_mut(),
            CREATE_NEW,
            FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            let e = GetLastError();
            let _ = ERROR_ALREADY_EXISTS; // documented reason: CREATE_NEW fails if it exists
            return Err(io::Error::from_raw_os_error(e as i32));
        }
        // Do NOT put this handle in a guard: on success we hand it to std::fs::File which owns/closes it.
        if let Err(e) = apply_and_verify_owner_only(handle, false) {
            CloseHandle(handle);
            return Err(e);
        }
        Ok(std::fs::File::from_raw_handle(handle as *mut _))
    }
}

/// Harden an EXISTING file we did not create atomically (e.g. `config.json`'s temp file written by std,
/// before its rename). Opens with reparse-open (does not traverse a reparse), applies the owner-only DACL.
pub fn harden_existing_file(path: &Path) -> io::Result<()> {
    harden_existing(path, false)
}

/// Harden an EXISTING directory (e.g. the persistent `prove-tmp` parent, or a `tempfile`-created child that
/// already inherits owner-only from its hardened parent). Inheritable so children stay private.
pub fn harden_existing_dir(path: &Path) -> io::Result<()> {
    harden_existing(path, true)
}

fn harden_existing(path: &Path, is_dir: bool) -> io::Result<()> {
    let w = wide(path);
    let mut flags = FILE_FLAG_OPEN_REPARSE_POINT;
    if is_dir {
        flags |= FILE_FLAG_BACKUP_SEMANTICS; // required to obtain a directory handle
    }
    unsafe {
        let handle = CreateFileW(
            w.as_ptr(),
            windows_sys::Win32::Storage::FileSystem::WRITE_DAC
                | windows_sys::Win32::Storage::FileSystem::READ_CONTROL
                | WRITE_OWNER,
            0,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            flags,
            std::ptr::null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return Err(last_err());
        }
        let _hg = HandleGuard(handle);
        apply_and_verify_owner_only(handle, is_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Temp root for one test's artifacts: a real `TempDir`, so cleanup happens on drop even on
    /// failure and parallel test binaries never share a name.
    fn scratch(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("aa-win-acl-{tag}-"))
            .tempdir()
            .expect("temp dir for win_acl test")
    }

    /// Open a path the way the readback helpers need (no reparse traversal), so a test can run
    /// `verify_owner_only` / `verify_owner_sid` against an object it did not just create. Sharing
    /// is fully open — an exclusive open would race antivirus/indexer handles and flake CI.
    fn open_for_readback(path: &Path, is_dir: bool) -> HANDLE {
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        let mut flags = FILE_FLAG_OPEN_REPARSE_POINT;
        if is_dir {
            flags |= FILE_FLAG_BACKUP_SEMANTICS;
        }
        unsafe {
            let handle = CreateFileW(
                wide(path).as_ptr(),
                windows_sys::Win32::Storage::FileSystem::READ_CONTROL,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                flags,
                std::ptr::null_mut(),
            );
            assert_ne!(handle, INVALID_HANDLE_VALUE, "open {path:?} for readback");
            handle
        }
    }

    /// Read the SD back and assert its DACL carries `SE_DACL_PROTECTED` — i.e. inherited parent
    /// ACEs were stripped, so "owner-only" cannot be silently widened by a future parent change.
    fn assert_dacl_protected(handle: HANDLE) {
        use windows_sys::Win32::Security::{GetSecurityDescriptorControl, SE_DACL_PROTECTED};
        unsafe {
            let mut sd: *mut core::ffi::c_void = std::ptr::null_mut();
            let rc = GetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut sd,
            );
            assert_eq!(rc, 0, "GetSecurityInfo for SD control");
            let _sd_guard = LocalFreeGuard(sd);
            let mut control: u16 = 0;
            let mut revision: u32 = 0;
            let ok = GetSecurityDescriptorControl(sd as _, &mut control, &mut revision);
            assert_ne!(ok, 0, "GetSecurityDescriptorControl");
            assert_ne!(
                control & SE_DACL_PROTECTED,
                0,
                "DACL must be PROTECTED (inherited ACEs stripped)"
            );
        }
    }

    /// True iff every ALLOW ACE on the object carries the `INHERITED_ACE` flag — this is what
    /// distinguishes "privilege flowed from the hardened parent" from "an equivalent explicit ACE".
    fn all_allow_aces_inherited(handle: HANDLE) -> bool {
        use windows_sys::Win32::Security::INHERITED_ACE;
        unsafe {
            let mut dacl: *mut ACL = std::ptr::null_mut();
            let mut sd: *mut core::ffi::c_void = std::ptr::null_mut();
            let rc = GetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut sd,
            );
            assert_eq!(rc, 0, "GetSecurityInfo for ACE flags");
            let _sd_guard = LocalFreeGuard(sd);
            assert!(!dacl.is_null());
            let count = (*dacl).AceCount as u32;
            assert!(count > 0, "child has no ACEs — inheritance did not flow");
            for i in 0..count {
                let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
                assert_ne!(GetAce(dacl, i, &mut ace), 0, "GetAce {i}");
                let header = &*(ace as *const ACE_HEADER);
                assert_eq!(header.AceType, ACCESS_ALLOWED_ACE_TYPE);
                if (header.AceFlags as u32) & INHERITED_ACE == 0 {
                    return false;
                }
            }
            true
        }
    }

    #[test]
    fn secure_create_dir_and_file_are_owner_only_and_usable() {
        let root = scratch("basic");
        let dir = root.path().join("secured");
        secure_create_dir(&dir).expect("secured dir (readback already asserted owner-only)");

        // The parent's PROTECTED-DACL property itself, read back off a second handle.
        let parent_handle = open_for_readback(&dir, true);
        let _phg = HandleGuard(parent_handle);
        assert_dacl_protected(parent_handle);
        drop(_phg);

        // A child created by plain std INSIDE the secured dir must INHERIT the owner-only ACE —
        // this is what makes the whole subtree private at creation, not just the top dir. The
        // assertion is on the INHERITED flag specifically, not merely on an equal-by-coincidence
        // explicit ACE.
        //
        // Deliberately NO owner assertion here: a std-created child's owner is the creating
        // TOKEN's default owner, which on an elevated context (e.g. the windows-latest runner)
        // is Administrators, not the current user. Ownership is a `secure_create_*` guarantee;
        // inheritance guarantees the DACL, and that is what is asserted.
        let child = dir.join("inherited.txt");
        std::fs::write(&child, b"secret").unwrap();
        let sid = current_user_sid().unwrap();
        let handle = open_for_readback(&child, false);
        let hg = HandleGuard(handle);
        unsafe {
            verify_owner_only(handle, &sid).expect("inherited DACL is owner-only");
        }
        assert!(
            all_allow_aces_inherited(handle),
            "child ACEs must carry INHERITED_ACE — privilege must come from the parent"
        );
        drop(hg);

        // Container inheritance: a plain child DIRECTORY must inherit too (CONTAINER_INHERIT_ACE),
        // and privacy must reach a grandchild file THROUGH it — this is what pins
        // SUB_CONTAINERS_AND_OBJECTS_INHERIT rather than an object-inherit-only regression that
        // would leave every nested directory world-readable-by-parent.
        let child_dir = dir.join("inherited-dir");
        std::fs::create_dir_all(&child_dir).unwrap();
        let sid2 = current_user_sid().unwrap();
        let cd_handle = open_for_readback(&child_dir, true);
        let cdg = HandleGuard(cd_handle);
        unsafe {
            verify_owner_only(cd_handle, &sid2).expect("child DIR inherits owner-only");
        }
        assert!(
            all_allow_aces_inherited(cd_handle),
            "child-DIR ACEs must carry INHERITED_ACE"
        );
        drop(cdg);
        let grandchild = child_dir.join("deep.txt");
        std::fs::write(&grandchild, b"deeper").unwrap();
        let gc_handle = open_for_readback(&grandchild, false);
        let gcg = HandleGuard(gc_handle);
        unsafe {
            verify_owner_only(gc_handle, &sid2).expect("grandchild owner-only through child dir");
        }
        assert!(all_allow_aces_inherited(gc_handle));
        drop(gcg);

        // The explicit-create file path: owner-only applied to the EMPTY file before bytes land.
        let file = root.path().join("explicit.bin");
        let mut f = secure_create_file(&file).expect("secured file");
        std::io::Write::write_all(&mut f, b"payload").unwrap();
        drop(f);
        assert_eq!(std::fs::read(&file).unwrap(), b"payload");
    }

    #[test]
    fn pre_planted_paths_are_rejected_not_adopted() {
        let root = scratch("preplant");

        // A file already at the target path: CREATE_NEW must fail closed.
        let file = root.path().join("planted.bin");
        std::fs::write(&file, b"attacker").unwrap();
        assert!(
            secure_create_file(&file).is_err(),
            "secure_create_file must reject an existing path"
        );

        // A directory already at the target path: CreateDirectoryW must fail closed.
        let dir = root.path().join("planted-dir");
        std::fs::create_dir_all(&dir).unwrap();
        assert!(
            secure_create_dir(&dir).is_err(),
            "secure_create_dir must reject an existing path"
        );
    }

    #[test]
    fn harden_existing_rejects_a_reparse_instead_of_securing_the_link() {
        let root = scratch("reparse");
        let real = root.path().join("real");
        std::fs::create_dir_all(&real).unwrap();
        let link = root.path().join("link");
        // A dir symlink IS a reparse point. Creating one needs SeCreateSymbolicLinkPrivilege:
        // GitHub's windows runners hold it (admin, UAC disabled) so CI must NEVER skip — a setup
        // regression there has to fail loudly. Only a local, privilege-less session may skip.
        if let Err(e) = std::os::windows::fs::symlink_dir(&real, &link) {
            const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;
            let privilege_missing = e.raw_os_error() == Some(ERROR_PRIVILEGE_NOT_HELD);
            if !(privilege_missing && std::env::var_os("GITHUB_ACTIONS").is_none()) {
                panic!("symlink_dir failed ({e}); the reparse test could not be set up");
            }
            eprintln!("skipping: symlink_dir unavailable without SeCreateSymbolicLinkPrivilege");
            return;
        }

        // harden_existing opens WITH FILE_FLAG_OPEN_REPARSE_POINT, so the handle IS the reparse;
        // reject_if_reparse must refuse it rather than ACL the link while accesses follow it.
        let err = harden_existing_dir(&link).expect_err("reparse must be refused");
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);

        // And the same refusal applies to the create path when the reparse appears first:
        // CreateDirectoryW sees it as an existing object.
        assert!(secure_create_dir(&link).is_err());
    }

    #[test]
    fn harden_existing_file_takes_on_a_plain_file_and_is_idempotent() {
        let root = scratch("harden");
        let file = root.path().join("plain.txt");
        std::fs::write(&file, b"data").unwrap();

        harden_existing_file(&file).expect("plain file hardens cleanly");
        // Second pass over an already-hardened object must stay Ok (idempotent belt).
        harden_existing_file(&file).expect("re-hardening is idempotent");

        let sid = current_user_sid().unwrap();
        let handle = open_for_readback(&file, false);
        let _hg = HandleGuard(handle);
        unsafe {
            verify_owner_only(handle, &sid).expect("hardened file is owner-only");
        }
    }
}
