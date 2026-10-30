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

Follow the [translation procedure](../../docs/localization.md). Keep rendering
and catalog invariants in `i18n` and Slint labels in the existing `Translations`
bridge, not a second locale mechanism.

- Treat safety wording as behavior. Preserve stopped, refused, unknown and
  completed outcomes, and the distinction between resolving evidence and
  printing. Never turn uncertainty into advice to retry or remove a
  confirmation to shorten a label.
- Translate presentation, not fiscal evidence. Preserve literal device/server
  detail, identifiers and entered receipt text. Never use translated UI strings
  as wire commands, persisted state names or API values.
- Apply the existing privacy policy to interpolated values. A translated error
  is not safe to log merely because its `Debug` is redacted.
