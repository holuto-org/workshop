---
description:
  "Novitus native wire text is Windows-1250 through fiscal::cp1250, never UTF-8 or Latin-1"
condition:
  - 'from_utf8|as_bytes\(\)|into_bytes\(\)|encoding_rs|latin'
scope: "tool:edit(crates/novitus/src/**), tool:write(crates/novitus/src/**)"
---

# Novitus wire encoding

Field text is Windows-1250: encode and decode it only through
`fiscal::cp1250::{encode, decode}` via `novitus::text`. Session bodies and
simulator replies use the same codec while permitting protocol CR separators;
text fields reject CR and every other control. UTF-8 or Latin-1 changes Polish
wire text and its XOR checksum. The five unassigned code-page bytes remain a
decode error, never replacement text. ASCII protocol identifiers and hex digits
are not text fields. Settings and journal JSON remain UTF-8; this rule concerns
only native protocol bytes. Device input/output code pages must be verified by
the future qualification layer; this transport never changes printer settings.
