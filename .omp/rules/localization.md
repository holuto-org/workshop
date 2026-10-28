---
description: "Safety-critical translation meaning and presentation boundaries"
condition: ".*"
scope:
  "tool:edit(crates/i18n/**), tool:write(crates/i18n/**),
  tool:edit(crates/desktop/ui/**), tool:write(crates/desktop/ui/**),
  tool:edit(crates/desktop/src/localization.rs),
  tool:write(crates/desktop/src/localization.rs),
  tool:edit(docs/localization.md), tool:write(docs/localization.md)"
interruptMode: never
---

# Localization

Follow the [translation procedure](../../docs/localization.md); rendering and
catalog invariants live in the `i18n` module, not a second locale mechanism.

- Treat safety wording as behavior. Every translation must preserve the
  distinction between stopped, refused, unknown and completed operations, and
  between resolving evidence and printing. Never soften uncertainty into a
  suggestion to retry or remove a confirmation to shorten a label.
- Translate presentation, not fiscal evidence. Preserve literal device/server
  detail, identifiers and entered receipt text; never reuse translated UI
  strings as wire commands, persisted state names or API values.
- Catalog validity is not linguistic or layout proof. Review changed meaning
  across languages and in the real control/dialog context; use the existing
  privacy policy when interpolating values. A translated error is not made
  safe for logging merely because its `Debug` is redacted.
