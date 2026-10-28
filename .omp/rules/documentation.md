---
description:
  "Documentation evidence, end-user contracts and incoming references"
condition: ".*"
scope:
  "tool:edit(**/*.md), tool:write(**/*.md), tool:edit(llms.txt),
  tool:write(llms.txt)"
interruptMode: never
---

# Documentation

Use [guidance homes](../../AGENTS.md#where-guidance-lives) for placement and the
public-source boundary, and
[finish checks](../../docs/workflows.md#finishing-a-change) for Markdown linting
and link checks.

- Read the implementing code path before describing behavior. Distinguish what
  the code does from what a specification requires or a simulator demonstrates;
  neither proves hardware qualification. Follow the
  [source and evidence guidance](../../docs/fiscal-contract.md#specification-sources)
  for vendor claims.
- `docs/install-with-ai.md` is the version-matched release guide; `llms.txt` is
  its discovery index, not a release asset or execution authority. Keep these
  end-user documents free of maintainer procedures. Check CLI examples, JSON
  fields and exit codes against `crates/desktop/src/cli.rs`, its called types
  and contract tests; check download instructions against the release workflow.
  Preserve their installation-versus-printing consent boundary.
- Before removing or renaming a documented item or heading, find its incoming
  references, including crate docs and absolute GitHub URLs in `llms.txt`.
  Update callers or preserve anchors that remain an external contract. A
  successful offline link check does not validate remote URLs, command behavior
  or release contents.
