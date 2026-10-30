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
trust boundaries. [Platform procedures](../../docs/testing/platforms.md#package-checks)
own package checks; [release YAML](../../.github/workflows/release.yml) owns the
native matrix and release commands.

- Never kill a running app or discard fiscal evidence to unblock an update.
  Preserve a recoverable previous installation when replacement fails.
- The legacy-journal update gate in the
  [install guide](../../docs/install-with-ai.md#3-install-on-the-detected-os)
  fails closed: parse the whole file, never grep for an accepted state, and
  never rewrite the evidence.
- Keep download integrity, provenance and OS approval distinct. Checksums do not
  prove release-workflow provenance. If `gh` exists, failed attestation stops
  installation; never remove it or fall back to checksums. If `gh` is absent,
  retain the documented checksum-only path with its explicit warning.
  Never strip quarantine/Mark-of-the-Web or bypass OS policy.
- Keep Unix installers compatible with macOS
  [Bash 3.2](https://github.com/apple-oss-distributions/bash) and BSD utilities,
  and Windows installers compatible with Windows PowerShell 5.1.
  Use `Set-Content -Stream Zone.Identifier`: .NET Framework file methods reject
  `file:stream` paths. Newer developer shells do not raise prerequisites.
- Separate local package checks from workflow steps that install system
  packages, upload symbols, attest or publish releases.
- Report exercised branches and skipped native checks. A dry run is not an
  installation; containers and cross-builds do not prove another OS's behavior.
- Changes to assets, prerequisites or installer behavior must keep the
  [install guide](../../docs/install-with-ai.md), [README](../../README.md#install),
  [LLM index](../../llms.txt) and release workflow consistent. Do not copy their
  procedures into this rule.
