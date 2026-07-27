#!/usr/bin/env bun
/**
 * F-008: report whether the LIVE @aztec/bb.js version has a REVIEWED Windows bb.exe pin.
 *
 * Run this AFTER `bun install` — it resolves the installed bb.js version (the SAME key the Windows
 * Prebuild/Build Smoke gate uses via resolveWindowsBbChecksum), never the argv/aztec.js version.
 *
 * Read-only: it NEVER downloads the asset or writes a pin. A pin is added by a human after review (see
 * WINDOWS_BB_CHECKSUMS in packages/accelerator/scripts/copy-bb.ts) — a twice-downloaded asset is not
 * independent evidence. This step is INFORMATIONAL; the enforcing fail-closed check is the Windows CI
 * gate (resolveWindowsBbChecksum throws without an accepted pin), and the only automated bump caller
 * (aztec-stable → main) runs that gate.
 */
import {
  resolveAztecBb,
  resolveWindowsBbChecksum,
  WINDOWS_BB_ASSET,
} from "../packages/accelerator/scripts/copy-bb.ts";

export interface WindowsBbPinStatus {
  version: string;
  present: boolean;
  message: string;
}

/** Report the pin status for `version` (defaults to the live bb.js version). Never touches the network. */
export function checkWindowsBbPin(version: string = resolveAztecBb().version): WindowsBbPinStatus {
  try {
    const sha = resolveWindowsBbChecksum(version);
    return {
      version,
      present: true,
      message: `✓ Windows bb.exe pin present for @aztec/bb.js ${version} (sha256:${sha.slice(0, 12)}…, provenance manual-review).`,
    };
  } catch (err) {
    const detail = err instanceof Error ? err.message : String(err);
    return {
      version,
      present: false,
      message:
        `⚠️  MANUAL PIN REQUIRED for @aztec/bb.js ${version}\n` +
        `The Windows Prebuild/Build Smoke CI gate stays RED until a human adds a reviewed pin.\n` +
        `Steps: download ${WINDOWS_BB_ASSET} from the v${version} aztec-packages release, verify the\n` +
        `release page + tag signature, diff it against the prior pinned asset, then add a\n` +
        `{ sha256, provenance: "manual-review", note } entry to WINDOWS_BB_CHECKSUMS in\n` +
        `packages/accelerator/scripts/copy-bb.ts. Pins are NEVER auto-generated (F-008).\n` +
        `(${detail})`,
    };
  }
}

if (import.meta.main) {
  const status = checkWindowsBbPin();
  console.log(status.message);
  // Informational only — exit 0 so the bump can still open its PR; the Windows CI gate is the enforcer.
  process.exit(0);
}
