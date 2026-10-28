# Maintaining translations

Use this procedure when adding or changing operator text. The
[`i18n` module](../crates/i18n/src/lib.rs) owns message rendering, diagnostic
output and catalog invariants; the
[desktop bridge](../crates/desktop/src/localization.rs) owns Slint integration.
User language selection belongs in the [README](../README.md#ui-language).

The shared catalogs are intentional: Slint's bundled translations do not expose
a public Rust lookup API in the pinned version
([upstream tracking](https://github.com/slint-ui/slint/issues/7949)). A separate
UI-only catalog would not replace the deferred worker notices.

## Change the message

1. Find the message ID and the action or error that displays it. Decide whether
   this is a translation correction or a change in meaning. For an uncertainty,
   confirmation or resolution warning, review the operator's resulting action
   against [fiscal safety](testing/fiscal-safety.md); a fluent translation must
   not turn a refusal into permission to retry or print.
2. Edit the existing domain under `crates/i18n/locales/<language>/`. A new
   message needs the same ID and argument names in every embedded language.
   A meaning change needs review of the corresponding translations; a typo in
   one language does not require unrelated edits to every catalog.
3. Keep device/server evidence, identifiers and customer-entered text as values,
   not catalog source. Use the message APIs documented by `i18n`; retain
   structured messages until the UI renders them. Do not translate protocol
   commands or locale-format fiscal values as part of a presentation change.
4. For Slint labels, use the existing `Translations` bridge. Check the real
   control and its accessible name, including an already-open dialog or tray
   menu when the language changes. Do not add a second translation mechanism.

## Verify the changed boundary

```sh
cargo nextest run -p i18n
```

The catalog tests check syntax, matching IDs/arguments and resolvable patterns.
They do not prove that a translation is correct or understandable. Review the
wording in context, especially the no-retry, uncertainty and consent warnings.

Run affected consumer tests too: a new driver error needs its driver scenario;
UI language-switch behavior uses the [real application fixture](testing/ui.md).
Check that existing input and warnings survive a switch, while diagnostic
output and simulated fiscal bytes retain their contracts. Exercise rendering
and layout against [simulators](../AGENTS.md#verify), never a real printer.
A text-only correction does not require rebuilding every native package.
