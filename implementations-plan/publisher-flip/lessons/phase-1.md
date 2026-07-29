# Lessons — publisher-flip

## "Only affects X" claims need the schema, not intuition

Plan fact 5 said publisher is Windows/NSIS-only. The tauri config schema says it also becomes the
Debian `Maintainer` when Cargo.toml has no authors (config.schema.json:2102). Harmless here (both
cosmetic changes wanted), but the claim was false and an audit caught it — same lesson as the
rename piece's "check the OLD code at the tag": authority beats recall.

## The cheapest insurance can be structurally worthless

The app-side registry mirror sounded like free belt-and-suspenders. Codex's rejection is the
keeper: it runs AFTER installation, so it cannot protect the very boundary it exists for — real
protection requires shipping in a PRIOR release, which is exactly the two-release migration the
empty fleet makes unnecessary. Insurance that cannot pay out is complexity, not safety.
