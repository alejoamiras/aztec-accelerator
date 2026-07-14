//! F-004 release-pipeline tool: assemble, splice, and verify the signed update-manifest envelope.
//!
//! The Tauri signer (`tauri signer sign`) does the actual minisign signing with the release key; this
//! tool only assembles the exact bytes to sign and, afterwards, embeds + verifies them — reusing the
//! SAME `accelerator_core::update_manifest` code the app runs, so the published feed is gated by the
//! production verifier before it ever reaches a user.
//!
//! Pipeline (see release-accelerator.yml):
//!   1. `update-manifest envelope --feed latest.json > envelope.json`
//!   2. `tauri signer sign envelope.json`            # → envelope.json.sig (base64 of the minisign doc)
//!   3. `update-manifest splice --feed latest.json --envelope envelope.json --sig envelope.json.sig \
//!         > latest.signed.json`
//!   4. `update-manifest verify --feed latest.signed.json --pubkey pubkey.b64`   # exit 0 ⇒ publishable
//!
//! Encoding contract (kept in lockstep with `verify_manifest`):
//!   - `manifest` = base64(envelope.json bytes); verify base64-decodes it then verifies the sig.
//!   - `manifest_sig` = the `.sig` file content VERBATIM. Tauri already writes it as base64(minisign
//!     doc), which is exactly what verify base64-decodes back into the minisign document.

use std::process::ExitCode;

use accelerator_core::update_manifest::{build_signed_envelope, verify_manifest};
use base64::Engine as _;

fn die(msg: String) -> ! {
    eprintln!("update-manifest: {msg}");
    std::process::exit(2);
}

fn read_to_string(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| die(format!("read {path}: {e}")))
}

fn read_json(path: &str) -> serde_json::Value {
    serde_json::from_str(&read_to_string(path))
        .unwrap_or_else(|e| die(format!("parse {path}: {e}")))
}

/// Value of `--flag <value>` from argv, or exit with a usage error.
fn flag(args: &[String], name: &str) -> String {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| die(format!("missing required {name}")))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("envelope") => {
            // Emit the canonical envelope bytes to sign, to stdout (binary-exact — no trailing newline).
            let feed = read_json(&flag(&args, "--feed"));
            match build_signed_envelope(&feed) {
                Ok(bytes) => {
                    use std::io::Write as _;
                    if let Err(e) = std::io::stdout().write_all(&bytes) {
                        die(format!("write stdout: {e}"));
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("envelope assembly failed: {e}");
                    ExitCode::from(1)
                }
            }
        }
        Some("splice") => {
            // Embed manifest + manifest_sig into the feed and print the result.
            let mut feed = read_json(&flag(&args, "--feed"));
            let env_bytes = std::fs::read(flag(&args, "--envelope"))
                .unwrap_or_else(|e| die(format!("read envelope: {e}")));
            let sig = read_to_string(&flag(&args, "--sig"));
            let b64 = base64::engine::general_purpose::STANDARD;
            feed["manifest"] = serde_json::Value::String(b64.encode(&env_bytes));
            // Verbatim: the .sig content is already base64(minisign doc), which is exactly what
            // verify base64-decodes. Re-encoding here would double-encode and fail verification.
            feed["manifest_sig"] = serde_json::Value::String(sig.trim().to_string());
            println!("{}", serde_json::to_string_pretty(&feed).unwrap());
            ExitCode::SUCCESS
        }
        Some("verify") => {
            // Gate: EVERY platform in the feed must bind through the production verifier.
            let feed = read_json(&flag(&args, "--feed"));
            let pubkey = read_to_string(&flag(&args, "--pubkey"));
            let version = feed["version"]
                .as_str()
                .unwrap_or_else(|| die("feed has no string `version`".into()));
            let platforms = feed["platforms"]
                .as_object()
                .unwrap_or_else(|| die("feed has no `platforms` object".into()));
            if platforms.is_empty() {
                die("feed has no platforms to verify".into());
            }
            let mut all_ok = true;
            for (target, p) in platforms {
                let url = p["url"].as_str().unwrap_or_default();
                let sig = p["signature"].as_str().unwrap_or_default();
                match verify_manifest(&feed, pubkey.trim(), version, url, sig) {
                    Ok(v) => println!("OK  {target}: v{} ({} bytes)", v.version, v.size),
                    Err(e) => {
                        eprintln!("FAIL {target}: {e}");
                        all_ok = false;
                    }
                }
            }
            if all_ok {
                println!(
                    "verify: all {} platform(s) bound to the signed envelope",
                    platforms.len()
                );
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        _ => {
            eprintln!("usage: update-manifest <envelope|splice|verify> [--flags]");
            ExitCode::from(2)
        }
    }
}
