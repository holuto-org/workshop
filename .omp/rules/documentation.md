---
description:
  "Public-repository documentation: one home per fact, no private or ignored
  sources, links that resolve"
condition: ".*"
scope:
  "tool:edit(**/*.md), tool:write(**/*.md), tool:edit(llms.txt),
  tool:write(llms.txt)"
interruptMode: never
---

# Documentation

This repository is public. Committed text describes this project only.

- Never reference gitignored paths (see `.gitignore`), except build output
  under `target/` named in commands, local-agent material (plans, specs, chat
  or review transcripts) or private sources (files and line numbers of the
  backend repository, vendor documents not linked publicly). Cite public URLs
  or describe the behavior.
- Each fact has one home ([AGENTS.md](../../AGENTS.md#where-guidance-lives)).
  Link to it instead of restating; when a change makes text untrue, fix its home
  in the same change. After removing or renaming an item, search its name in
  `AGENTS.md`, `README.md`, `docs`, `.omp`, `llms.txt` and crate `//!` docs.
- Write a behavior claim only after reading the code path that implements it;
  mark spec-only or unmeasured claims as unconfirmed.
- `docs/install-with-ai.md` and `llms.txt` are end-user material shipped with
  releases: keep CLI commands, JSON fields and exit codes exactly in sync with
  `crates/desktop/src/cli.rs`, and keep maintainer notes out of them.
- Adding, renaming or removing a rule updates the rule index in `AGENTS.md`.
- Every relative link and `[ref]: path` must resolve; keep README anchors used by
  `llms.txt` and other docs stable.
- Markdown passes `npx -y markdownlint-cli2 <files>` with the default rules
  (80 columns; prefer bullets to wide tables). Fix the text, never the config.
- Check local links and anchors with
  `lychee --offline --include-fragments --no-progress <files>`.
