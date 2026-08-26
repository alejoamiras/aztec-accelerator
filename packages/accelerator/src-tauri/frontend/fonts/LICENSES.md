# Vendored font licenses

The woff2 files in this directory are unmodified builds vendored from Fontsource npm packages
(which package the upstream Google Fonts releases). All three families are licensed under the
SIL Open Font License 1.1 — full text in `OFL-1.1.txt` beside this file, satisfying the OFL's
redistribution condition. Per-file sha256 integrity is pinned in
`scripts/brand-assets.sha256.json` and enforced by `scripts/icon-assets.test.ts`; re-vendor with
`bun scripts/generate-brand-assets.ts --target fonts`.

| Family | Upstream package | Copyright |
|---|---|---|
| Bricolage Grotesque | `@fontsource/bricolage-grotesque@5.3.0` | © 2020 The Bricolage Grotesque Project Authors (https://github.com/ateliertriay/bricolage) |
| Figtree | `@fontsource/figtree@5.3.0` | © 2022 The Figtree Project Authors (https://github.com/erikdkennedy/figtree) |
| Fragment Mono | `@fontsource/fragment-mono@5.3.0` | © 2022 The Fragment Mono Project Authors (https://github.com/weiweihuanghuang/fragment-mono) |
