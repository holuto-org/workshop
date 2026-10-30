# Maintaining translations

Use this procedure when adding or changing operator text. The
[`i18n` module](../crates/i18n/src/lib.rs) owns message rendering, diagnostic
output and catalog invariants; the
[desktop bridge](../crates/desktop/src/localization.rs) owns Slint integration.
User language selection belongs in the [README](../README.md#ui-language).

The shared catalogs are intentional: Slint's bundled translations do not expose
a public Rust lookup API in the locked Slint 1.18.1 release
([upstream tracking](https://github.com/slint-ui/slint/issues/7949)). A UI-only
catalog would not cover deferred worker notices.

## Change the message

1. Find the message ID and the action or error that displays it. Decide whether
   this is a translation correction or a change in meaning. For an uncertainty,
   confirmation or resolution warning, review the operator's resulting action
   against [fiscal safety](testing/fiscal-safety.md); a fluent translation must
   not turn a refusal into permission to retry or print.
2. Edit the existing domain under `crates/i18n/locales/<language>/`. Prefix new
   IDs with the domain name and use the same IDs and argument names in `en`,
   `pl`, `es` and `ru`.
   A meaning change needs review of the corresponding translations; a typo in
   one language does not require unrelated edits to every catalog.
3. Keep device/server evidence, identifiers and customer-entered text as values,
   not catalog source. Use `Message::new(...).arg(...)` for application text and
   `Message::literal(...)` for external detail. Retain nested messages until
   `Message::render(language)` at the UI boundary. `Display` and `to_string()`
   produce Polish diagnostics, not selected-language UI text. Render those
   separately for journal/API output. Do not translate protocol commands or
   locale-format fiscal values as part of a presentation change.
4. For Slint labels, use the existing `Translations` bridge. Check the real
   control and its accessible name, including an already-open dialog or tray
   menu when the language changes. Do not add a second translation mechanism.
   Recovery instructions must name the translated action the operator can
   actually choose; catalogs can reference that action's message ID.
   Toolkit-owned text, such as input context menus, is outside these catalogs:
   check it separately and use the toolkit's supported localization path
   before changing it.

## Verify the changed boundary

```sh
cargo nextest run -p i18n
```

The catalog tests check syntax, matching IDs/arguments and resolvable patterns.
They do not prove that a translation is correct or understandable. Review the
wording in context, especially the no-retry, uncertainty and consent warnings.

Run affected consumer tests too: a new driver error needs its driver scenario.
For nested failure reasons, drive both validation rejection and simulator
refusal through the worker, render the resulting notice in all four languages,
and check that journal/API diagnostics stay Polish.

Use the [real application fixture](testing/ui.md) for language-switch behavior.
Set its starting language explicitly so the runner's locale cannot change
control lookup, and keep existing behavior assertions when localizing a test.
Preserve typed input and the selected report period while updating month
labels, warnings, dialogs and tray text; report-month tests keep the 24-month
range and the selection across tab switches. Check keyboard focus
during preference saving: Space must not toggle diagnostic consent, Return
must reopen the language choices, and a second selection must persist.

Exercise layout with a real renderer against
[simulators](../AGENTS.md#verify), never a real printer. Slint's testing backend
does not render pixels, so passing interaction tests cannot prove text fits.
Check that simulated fiscal bytes retain their contracts. A text-only
correction does not require rebuilding every native package.
