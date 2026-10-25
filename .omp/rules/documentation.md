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

- Never reference gitignored paths (see `.gitignore`), local-agent material
  (plans, specs, chat or review transcripts) or private sources (files and line
  numbers of the backend repository, vendor documents not linked publicly).
  Cite public URLs or describe the behavior.
- Each fact has one home ([AGENTS.md](../../AGENTS.md#where-guidance-lives)).
  Link to it instead of restating; when a change makes text untrue, fix its home
  in the same change.
- `docs/install-with-ai.md` and `llms.txt` are end-user material shipped with
  releases: keep CLI commands, JSON fields and exit codes exactly in sync with
  `crates/desktop/src/cli.rs`, and keep maintainer notes out of them.
- Adding, renaming or removing a rule updates the rule index in `AGENTS.md`.
- Every relative link and `[ref]: path` must resolve; keep README anchors used by
  `llms.txt` and other docs stable.
