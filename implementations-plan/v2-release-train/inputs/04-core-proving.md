# Core server / proving hot path (agent 4)

CORRECTION to my prior framing: the bb.rs "unbounded stderr buffer" I've cited since #434 is WRONG.

1. **[P1] bb::prove never pipes stdout/stderr — diagnostics silently lost, dead code.** bb.rs:206-229
   spawns bb WITHOUT .stdout/.stderr(Stdio::piped()). Verified vs vendored tokio-1.52.3
   process/mod.rs:1442: output.stderr/stdout (bb.rs:230,238) are ALWAYS empty. So warn!("bb stderr")
   (:239-241) and truncate_stderr (:268-280, unit-tested pure only, never through a real spawn) are
   DEAD CODE. A `bb prove failed (exit N)` in prod is close to undiagnosable — only an exit code. The
   task's "retains whole stderr" premise doesn't hold; stderr is INHERITED to parent fd, bypassing the
   app's tracing/redaction. Fix is NOT just add Stdio::piped() — that reinstates the unbounded buffer.
   Pipe with a size-capped reader (CappedReader already exists, downloader.rs:339-360), e.g. 64KB.
   Small, high value for v2 supportability.
2. **[P2] Witness workspace has no crash-orphan reaper.** create_prove_tempdir/prove_tmp_parent
   (bb.rs:77-153) rely solely on TempDir Drop. Covers in-process failure; does NOT cover parent
   process dying uncleanly (SIGKILL/OOM/power loss) mid-proof — Drop never runs, witness (0600) sits
   indefinitely. downloader.rs:232-256 has the age-gated reap pattern, never applied to prove-tmp.
   [NOTE: F-08a added a startup reaper on the bind win — verify whether this P2 is already closed or a
   real remaining gap; agent may not have seen the merged F-08a work.] Small.
3. **[P3] No cap on Mainnet-tier version cache growth, reachable by an approved origin.**
   version_policy.rs:53 Mainnet retention_limit()->None; no global byte budget. Digest-verified so not
   unauth-payload DoS, but an approved origin walks x-aztec-version through every historical mainnet
   release, each cached forever → tens of GB, no eviction. Small — give Mainnet a retention count / LRU
   ceiling.
4. **[P4] No sanity check on bb proof output before 200.** bb.rs:250-255 base64s output/proof on exit
   0; empty/truncated file returns "successful" 4-byte response. Cheap: reject empty/non-32B-aligned.

Well-handled (verified): prove permit/waiter split bounds memory to MAX_INFLIGHT_PROVE×50MB; download
caps 64MB/512MB streaming; marker-verify-before-execute (F-007) fail-closed; loopback Host guard,
RFC-6454 origin canon, single-popup arbiter mature; stage-reap races age-gated; dual-instance fails
closed.
