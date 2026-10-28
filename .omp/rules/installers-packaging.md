---
description:
  "Installer trust/fiscal boundaries and native package compatibility"
condition: ".*"
scope:
  "tool:edit(scripts/*install.*), tool:write(scripts/*install.*),
  tool:edit(packaging/**), tool:write(packaging/**),
  tool:edit(.github/workflows/checks.yml), tool:write(.github/workflows/checks.yml),
  tool:edit(.github/workflows/release.yml), tool:write(.github/workflows/release.yml),
  tool:edit(.cargo/config.toml), tool:write(.cargo/config.toml),
  tool:edit(Cargo.toml), tool:write(Cargo.toml)"
interruptMode: never
---

# Installers and packaging

The [install guide](../../docs/install-with-ai.md) owns installer behavior and
trust boundaries;
[platform procedures](../../docs/testing/platforms.md#package-checks) own package
checks. [Workflow YAML](../../.github/workflows/release.yml) owns the native
matrix and release commands.

- A blocked update is not permission to kill a running app, discard fiscal
  evidence, or interpret an unreadable/malformed legacy journal as resolved.
  Preserve a recoverable previous installation when replacement fails.
- Keep download integrity distinct from provenance and OS approval. A checksum
  is not proof of the release workflow; failed attestation must not silently
  fall back to checksums. Do not strip quarantine/Mark-of-the-Web or bypass OS
  policy to make installation succeed.
- Use the target's stock tools: the Unix installer must work with macOS
  [Bash 3.2](https://github.com/apple-oss-distributions/bash) and BSD utilities;
  Windows installer changes must work in Windows PowerShell 5.1, not only pwsh.
  A newer shell on a developer machine is not a reason to raise prerequisites.
- Use isolated unpaired state for package launch checks, never saved user state.
  Follow the platform procedures for Windows Known Folders and Unix isolation.
  Separate local build/verification from workflow steps that install system
  packages, upload symbols, attest or publish releases.
- Native checks prove OS behavior; containers and dry runs prove only their
  exercised branches. Report skipped branches and do not present a dry run as
  installation or a cross-build as native validation.
- Changes to assets, prerequisites or installer behavior must keep the
  [install guide](../../docs/install-with-ai.md), [README](../../README.md#install),
  [LLM index](../../llms.txt) and release workflow consistent. Do not copy their
  procedures into this rule.
