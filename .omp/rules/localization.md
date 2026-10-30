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

1. Find the message ID and the action that displays it. Add new IDs with matching
   argument names to every language under `crates/i18n/locales/`. Review all
   corresponding translations when meaning changes. Keep a spelling-only fix
   in the affected language.
2. Treat safety wording as behavior. Preserve stopped, refused, unknown and
   completed outcomes, and the distinction between resolving evidence and
   printing. Never turn uncertainty into advice to retry or remove a
   confirmation to shorten a label.
3. Translate presentation, not fiscal evidence. Preserve literal device/server
   detail, identifiers and entered receipt text. Never use translated UI strings
   as wire commands, persisted state names or API values.
4. Check changed text in its real control and accessible name. Check
   toolkit-owned text, including input context menus, separately from app
   labels; verify the toolkit's supported localization path before changing it.
   For language-switch changes, switch with dialogs and tray menus already open;
   check their text and accessible names. Catalog validity proves neither
   wording nor layout.
5. Keep existing behavior assertions when localizing tests. Set an explicit
   language for language-specific expectations instead of dropping coverage.
   For report-month changes, retain the 24-month range and tab-capture checks;
   verify that switching languages changes labels, not the selected period.
6. Apply the existing privacy policy to interpolated values. A translated error
   is not safe to log merely because its `Debug` is redacted.
