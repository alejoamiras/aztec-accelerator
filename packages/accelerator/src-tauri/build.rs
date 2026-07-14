/// FNV-1a 64-bit over raw bytes → lowercase hex, padded to 16. MUST match `scripts/build-frontend.ts`
/// so the staleness check compares like-for-like (a content fingerprint, not a security primitive).
fn fnv1a64_hex(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// F-012: the popup frontend ships as gitignored `frontend/assets/*.js` bundled from `frontend-src/` by
/// `bun run frontend:build`. Those bundles are trusted by `script-src 'self'`, so a Rust build must NEVER
/// silently embed HTML pointing at MISSING or STALE bundles. This fails the build (with a fix hint) unless
/// every source file's current fingerprint matches the `.build-manifest.json` written at bundle time.
fn verify_frontend_bundles() {
    println!("cargo:rerun-if-changed=frontend-src");
    println!("cargo:rerun-if-changed=frontend/assets/.build-manifest.json");

    let hint = "run `bun run --cwd packages/accelerator frontend:build` before building the Tauri app \
                (the bundles are gitignored; tauri's beforeDev/beforeBuildCommand does this automatically)";

    for bundle in ["authorize.js", "settings.js", "update-prompt.js"] {
        let p = format!("frontend/assets/{bundle}");
        println!("cargo:rerun-if-changed={p}");
        if !std::path::Path::new(&p).is_file() {
            panic!("F-012: missing frontend bundle `{p}` — {hint}");
        }
    }

    let manifest_path = "frontend/assets/.build-manifest.json";
    let manifest_raw = std::fs::read_to_string(manifest_path)
        .unwrap_or_else(|e| panic!("F-012: cannot read {manifest_path}: {e} — {hint}"));
    let manifest: serde_json::Value = serde_json::from_str(&manifest_raw)
        .unwrap_or_else(|e| panic!("F-012: {manifest_path} is not valid JSON: {e} — {hint}"));
    let recorded = manifest
        .get("inputs")
        .and_then(|v| v.as_object())
        .unwrap_or_else(|| panic!("F-012: {manifest_path} has no `inputs` object — {hint}"));

    // Every current source file must be present in the manifest with a matching fingerprint.
    let mut seen = 0usize;
    for entry in std::fs::read_dir("frontend-src")
        .unwrap_or_else(|e| panic!("F-012: cannot read frontend-src/: {e}"))
    {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("js") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap()
            .to_string();
        let actual = fnv1a64_hex(&std::fs::read(&path).expect("read source"));
        let expected = recorded.get(&name).and_then(|v| v.as_str()).unwrap_or_else(|| {
            panic!("F-012: source `frontend-src/{name}` is missing from the bundle manifest (STALE) — {hint}")
        });
        if actual != expected {
            panic!("F-012: `frontend-src/{name}` changed since the bundles were built (STALE) — {hint}");
        }
        seen += 1;
    }
    // And the manifest must not reference sources that no longer exist (also stale).
    if seen != recorded.len() {
        panic!("F-012: the bundle manifest references {} sources but frontend-src/ has {seen} — {hint}",
            recorded.len());
    }
}

fn main() {
    verify_frontend_bundles();

    // Re-run when AZTEC_VERSION changes (or is created/deleted)
    println!("cargo:rerun-if-changed=AZTEC_VERSION");

    // Expose bundled Aztec bb version at compile time
    if let Ok(version) = std::fs::read_to_string("AZTEC_VERSION") {
        println!("cargo:rustc-env=AZTEC_BB_VERSION={}", version.trim());
    } else {
        println!("cargo:rustc-env=AZTEC_BB_VERSION=unknown");
    }

    // Build-time syntax check for verified-sites.json so malformed JSON
    // fails the cargo build instead of bricking installed users at startup.
    println!("cargo:rerun-if-changed=../verified-sites.json");
    let vs_path = "../verified-sites.json";
    let vs_contents = std::fs::read_to_string(vs_path)
        .unwrap_or_else(|e| panic!("verified-sites.json missing or unreadable at {vs_path}: {e}"));
    serde_json::from_str::<serde_json::Value>(&vs_contents)
        .unwrap_or_else(|e| panic!("verified-sites.json is not valid JSON: {e}"));

    // F-012: declare the app's IPC command surface. Tauri only enforces the per-window ACL for app-local
    // commands when an app manifest exists (see tauri `webview/mod.rs`: the invoke gate is
    // `plugin_command || has_app_acl_manifest`). Declaring these commands sets `has_app_acl` true, which
    // flips every one of them from framework-default-allow to per-window default-DENY: a window whose
    // capability does not grant `allow-<command>` is rejected before the handler runs. This is ALL-OR-
    // NOTHING — every command below MUST be granted by exactly the windows that use it (see
    // capabilities/*.json), or that flow breaks. Keep this list == the generate_handler! set in main.rs
    // (the scripts/tauri-trust-boundary.test.ts set-equality guard fails CI on drift).
    let commands: &[&str] = &[
        "get_config",
        "get_autostart_enabled",
        "set_autostart",
        "set_speed",
        "remove_approved_origin",
        "get_system_info",
        "get_verified_info",
        "respond_auth",
        "enable_safari_support",
        "disable_safari_support",
        "set_auto_update",
        "respond_update_prompt",
    ];
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(commands)),
    )
    .expect("failed to run tauri-build");
}
