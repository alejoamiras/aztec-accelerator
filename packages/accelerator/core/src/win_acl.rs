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
#![cfg(windows)]

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
    DACL_SECURITY_INFORMATION, NO_INHERITANCE, PROTECTED_DACL_SECURITY_INFORMATION, PSID,
    SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_QUERY, TOKEN_USER,
};

/// `ACCESS_ALLOWED_ACE_TYPE` (winnt.h `0x0`) — not re-exported by windows-sys under this path, so pinned
/// to its documented value. Only this ACE type has the `SidStart` layout `verify_owner_only` reads.
const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
use windows_sys::Win32::Storage::FileSystem::{
    CreateDirectoryW, CreateFileW, CREATE_NEW, FILE_ALL_ACCESS, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, OPEN_EXISTING,
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
        let token_user = &*(buf.as_ptr() as *const TOKEN_USER);
        let sid_ptr: PSID = token_user.User.Sid;
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
    let rc = SetSecurityInfo(
        handle,
        SE_FILE_OBJECT,
        DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        acl,
        std::ptr::null_mut(),
    );
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc as i32));
    }

    // Fail-closed readback: catches FAT/exFAT / network volumes that silently ignore ACLs.
    verify_owner_only(handle, &sid)
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
        // Open a handle to the just-created directory to apply the DACL.
        let handle = CreateFileW(
            w.as_ptr(),
            windows_sys::Win32::Storage::FileSystem::WRITE_DAC
                | windows_sys::Win32::Storage::FileSystem::READ_CONTROL,
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
                | windows_sys::Win32::Storage::FileSystem::READ_CONTROL,
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
                | windows_sys::Win32::Storage::FileSystem::READ_CONTROL,
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
