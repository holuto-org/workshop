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

- Use the definitions for the actual model, protocol and operating mode.
  Shared command names do not make legacy behavior applicable to ONLINE
  printers or one brand's completion semantics applicable to another.
- Separate vendor guarantees, conservative host policy and measured hardware.
  Do not present parser restrictions, limits or deadlines as vendor mandates.
  Simulator fixtures and candidate model names do not qualify hardware; use
  the [qualification procedure](../../docs/testing/hardware-acceptance.md)
  before enabling unqualified capabilities.
- Keep command acceptance, completed printing and proof of refusal distinct.
  An acknowledgement or a counter change alone is not completion evidence;
  Elzab daily `RRP` is a concrete acceptance-only case. Changes to certainty
  must satisfy
  [recovery classification](../../docs/testing/fiscal-safety.md#recovery-classification),
  not merely a successful simulator reply or a vendor error name.
- Record an unmeasured discrepancy as an
  [open hardware question](../../docs/testing/hardware-acceptance.md#open-hardware-questions),
  not a confirmed device bug. Do not change fiscal behavior to resolve an
  unsupported assumption, or weaken a safety check to make a fixture pass.
