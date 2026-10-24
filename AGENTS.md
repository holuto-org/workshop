# Repo Rules

## Scope

- Before edit or verify: collect every `AGENTS.md` from repo root to changed files.
- Rules add together across chain.

## Layout

One Cargo workspace; dependency versions and lints live only in the root `Cargo.toml`, crates use
`workspace = true`.

| Crate | Owns |
| --- | --- |
| `crates/posnet` | Posnet Temo Online driver: CP1250 text, frames, sessions, preflight, receipts, reports. Feature `simulator`: the in-process TCP peer every printer test uses. |
| `crates/agent` | Holuto `/agent` API, the durable operation journal, receipt execution and recovery, settings, history, and the single printer `Worker` thread. |
| `crates/desktop` | `holuto-fiscal`: Slint UI and tray. Feature `simulator`: runs against the in-process simulator with separate state, for local smoke runs, never in release builds. |

## Code

- A justified lint exception is `#[expect(lint, reason = "…")]` at the call site, never `allow`.
- Modules are `foo.rs` + `foo/`, never `mod.rs`; unit tests live in `foo/tests.rs`.
- No `pub use`: import items from the module that defines them.
- Errors callers match on are `thiserror` enums; their `Display` is the Polish, operator-facing text.

## Tooling

- Prefer `rtk cargo …`; no `rtk` installed → plain `cargo …`.

## Verify

Subagents: skip checks below.

Run all commands from the repo root. After code edits:

```sh
cargo +nightly-2026-10-06 fmt   # pinned nightly: .rustfmt.toml uses unstable options; builds stay on stable
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --workspace --all-features
```

Before finishing dependency changes: `cargo deny --locked check` and `cargo shear --deny-warnings`.
