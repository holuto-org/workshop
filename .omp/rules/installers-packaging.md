---
description:
  "Installer safety, portable scripts, package checks and local CI limits"
condition: ".*"
scope:
  "tool:edit(scripts/**), tool:write(scripts/**),
  tool:edit(packaging/**), tool:write(packaging/**),
  tool:edit(.github/workflows/**), tool:write(.github/workflows/**)"
interruptMode: never
---

# Installers and packaging

Use [platform procedures](../../docs/testing/platforms.md) and
[CI](../../docs/ci.md); workflow YAML owns the commands and native matrix.

- Run the changed area's commands from
  [Checks](../../.github/workflows/checks.yml) and the Package job in
  [Release](../../.github/workflows/release.yml); retain its
  simulator-release rejection check. Native package procedures live in
  [platforms](../../docs/testing/platforms.md#package-checks).
- If ShellCheck is missing, use `uvx --from shellcheck-py shellcheck` with the
  same arguments; a PowerShell container can run non-Windows fixture branches.
  Neither substitutes for native Windows/macOS coverage or release publishing.
- On an unsupported distribution (such as Fedora) the installer dry run exits 1
  by design. Full Linux fixtures need Ubuntu 24.04+/Debian 13+ and a non-root
  user, including inside containers. macOS replacement fixtures skip without
  `/usr/libexec/PlistBuddy`.
- Build packages outside a concurrently edited tree; `git archive HEAD` copies
  committed files only. Check dependencies and memory before starting; do not
  weaken the release profile to fit a small container.
- SELinux Rust cache mounts: use `--security-opt label=disable`, never `:Z`.
- Keep intentional protections: exact manifest SHA-256 and workflow/branch-bound
  attestation; downgrade, running-app and unresolved HuggingCar Fiscal journal
  refusal; macOS previous-app recovery; existing required-command checks; ELF
  dependency-to-Debian-package mapping; Windows PE import walk rejecting dynamic
  VC runtime dependencies.
- Keep one package `gh attestation verify` after download/hash verification,
  without auth/version preflights. Failure stops installation; only absent
  `gh` permits checksum-only installation with an explicit warning.
- Preserve macOS system Bash 3.2/BSD-tool compatibility: no `${x,,}` or
  `sort -V`; keep `version_gt` numeric and compare `wc -l` counts numerically.
  Preserve Windows PowerShell 5.1 support, not just pwsh.
- Release asset or installer behavior changes must keep the
  [install guide](../../docs/install-with-ai.md), [README](../../README.md#install),
  [LLM index](../../llms.txt) and release workflow aligned.
- Describe what the installer user sees: files changed, permission/security
  prompts and when it stops. Report missing tools and skipped native checks;
  never call a dry run proof of installation or claim removed checks remain.
