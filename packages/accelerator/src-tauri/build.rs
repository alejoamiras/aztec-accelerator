/// SHA-256 over raw bytes → lowercase hex. MUST match `scripts/build-frontend.ts` so the guard compares
/// like-for-like (GATE-3 codex: strong enough to detect a swapped OUTPUT bundle, not just staleness).
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

/// Read `path`, compare its SHA-256 to the manifest entry `key`; panic (with `hint`) on missing/mismatch.
fn verify_hash(
    path: &str,
    key: &str,
    recorded: &serde_json::Map<String, serde_json::Value>,
    hint: &str,
) {
    println!("cargo:rerun-if-changed={path}");
    let bytes = std::fs::read(path).unwrap_or_else(|e| {
        panic!("F-012: cannot read `{path}` (manifest key `{key}`): {e} — {hint}")
    });
    let expected = recorded
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            panic!("F-012: `{key}` missing from the bundle manifest (STALE) — {hint}")
        });
    if sha256_hex(&bytes) != expected {
        panic!("F-012: `{path}` does not match the bundle manifest (`{key}` STALE or SWAPPED) — {hint}");
    }
}

/// F-012: the popup frontend ships as gitignored `frontend/assets/*.js` bundled from `frontend-src/` by
/// `bun run frontend:build`. Those bundles are trusted by `script-src 'self'`, so a Rust build must NEVER
/// silently embed HTML pointing at MISSING, STALE, or SWAPPED bundles. This fails the build unless the
/// current sources, the dependency-surface files, AND the emitted bundles all match `.build-manifest.json`.
fn verify_frontend_bundles() {
    println!("cargo:rerun-if-changed=frontend-src");
    println!("cargo:rerun-if-changed=frontend/assets/.build-manifest.json");

    let hint = "run `bun run --cwd packages/accelerator frontend:build` before building the Tauri app \
                (the bundles are gitignored; tauri's beforeDev/beforeBuildCommand does this automatically)";

    let manifest_path = "frontend/assets/.build-manifest.json";
    let manifest_raw = std::fs::read_to_string(manifest_path)
        .unwrap_or_else(|e| panic!("F-012: cannot read {manifest_path}: {e} — {hint}"));
    let manifest: serde_json::Value = serde_json::from_str(&manifest_raw)
        .unwrap_or_else(|e| panic!("F-012: {manifest_path} is not valid JSON: {e} — {hint}"));
    if manifest.get("algo").and_then(|v| v.as_str()) != Some("sha256") {
        panic!("F-012: {manifest_path} algo is not sha256 — {hint}");
    }
    let inputs = manifest
        .get("inputs")
        .and_then(|v| v.as_object())
        .unwrap_or_else(|| panic!("F-012: {manifest_path} has no `inputs` object — {hint}"));
    let outputs = manifest
        .get("outputs")
        .and_then(|v| v.as_object())
        .unwrap_or_else(|| panic!("F-012: {manifest_path} has no `outputs` object — {hint}"));

    // Emitted bundles: exact set + content match (catches a post-build swap).
    let bundles = ["authorize.js", "settings.js", "update-prompt.js"];
    if outputs.len() != bundles.len() {
        panic!(
            "F-012: manifest lists {} output bundles, expected {} — {hint}",
            outputs.len(),
            bundles.len()
        );
    }
    for bundle in bundles {
        verify_hash(&format!("frontend/assets/{bundle}"), bundle, outputs, hint);
    }

    // Dependency-surface inputs (a @tauri-apps/api bump must invalidate the bundles).
    verify_hash("../package.json", "package.json", inputs, hint);
    verify_hash("../../../bun.lock", "bun.lock", inputs, hint);

    // Every current frontend source must match; the manifest must reference no phantom sources.
    for entry in std::fs::read_dir("frontend-src")
        .unwrap_or_else(|e| panic!("F-012: cannot read frontend-src/: {e}"))
    {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("js") {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap();
        verify_hash(
            &format!("frontend-src/{name}"),
            &format!("frontend-src/{name}"),
            inputs,
            hint,
        );
    }
    let src_keys = inputs
        .keys()
        .filter(|k| k.starts_with("frontend-src/"))
        .count();
    let on_disk = std::fs::read_dir("frontend-src")
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("js"))
        .count();
    if src_keys != on_disk {
        panic!("F-012: manifest lists {src_keys} frontend sources but frontend-src/ has {on_disk} — {hint}");
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
