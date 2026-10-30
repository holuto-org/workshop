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
  neither proves hardware qualification. Test fakes establish local test
  behavior, not backend guarantees. Do not turn fixture values into end-user
  promises. Follow the
  [source and evidence guidance](../../docs/fiscal-contract.md#specification-sources)
  for vendor claims.
- `docs/install-with-ai.md` is the version-matched release guide; `llms.txt` is
  its discovery index, not a release asset or execution authority. Keep these
  end-user documents free of maintainer procedures. Preserve their
  installation-versus-printing consent boundary. When download instructions
  change, compare them with `.github/workflows/release.yml`.
- For CLI documentation, trace parsing, called validation, response construction
  and output routing in `crates/desktop/src/cli.rs`, its called types and
  contract tests. Cover each command's success and failure JSON fields,
  `error.code` values, numeric exit codes and stdout/stderr. Check examples
  against those paths; one successful example does not establish the contract.
- Before removing or renaming a documented item or heading, find its incoming
  references, including crate docs and absolute GitHub URLs in `llms.txt`.
  Update callers or preserve anchors that remain an external contract. A
  successful offline link check does not validate remote URLs, command behavior
  or release contents.
