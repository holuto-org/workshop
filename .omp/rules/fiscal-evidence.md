---
description: "Vendor evidence, host policy and fiscal certainty across printer brands"
condition: ".*"
scope:
  "tool:edit(crates/posnet/**), tool:write(crates/posnet/**),
  tool:edit(crates/novitus/**), tool:write(crates/novitus/**),
  tool:edit(crates/elzab/**), tool:write(crates/elzab/**),
  tool:edit(crates/fiscal/**), tool:write(crates/fiscal/**),
  tool:edit(crates/agent/src/agent*), tool:write(crates/agent/src/agent*),
  tool:edit(crates/agent/src/agent/**), tool:write(crates/agent/src/agent/**)"
interruptMode: never
---

# Fiscal evidence

Before changing wire, preflight, profiles or recovery certainty, read the
owning module docs and the
[public protocol sources](../../docs/fiscal-contract.md#specification-sources).

- Match the actual model, protocol revision and operating mode. Cite the
  document, revision and command or section behind a claim. Shared names do
  not transfer legacy behavior to ONLINE printers or completion semantics
  between brands.
- Label vendor guarantees, host policy and hardware observations separately.
  Label uncited parser restrictions, limits and deadlines as host policy,
  not vendor mandates. Simulator fixtures and candidate model names do not
  qualify hardware. Before enabling unqualified capabilities, obtain scoped,
  supervised evidence through the
  [qualification procedure](../../docs/testing/hardware-acceptance.md).
- Distinguish acceptance, completed printing and proven refusal. Elzab daily
  `RRP` proves acceptance only; ACK, idle status and advancing counters do not
  prove completion. Before changing certainty, satisfy
  [recovery classification](../../docs/testing/fiscal-safety.md#recovery-classification).
  A passing simulator reply or an error-table entry alone cannot prove that
  the device had no effects.
- When behavior depends on unmeasured hardware, preserve the existing fiscal
  behavior and record a suspected discrepancy under
  [open hardware questions](../../docs/testing/hardware-acceptance.md#open-hardware-questions).
  Missing proof is not a confirmed device bug or permission to remove an
  existing certainty exception. Do not weaken a safety check for a fixture.
