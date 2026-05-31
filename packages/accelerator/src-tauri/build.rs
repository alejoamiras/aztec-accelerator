fn main() {
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

    tauri_build::build()
}
