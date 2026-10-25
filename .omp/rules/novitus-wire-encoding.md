---
description:
  "Novitus native wire text is Windows-1250 through fiscal::cp1250, never UTF-8 or Latin-1"
condition:
  - 'from_utf8|as_bytes\(\)|into_bytes\(\)|encoding_rs|latin'
scope: "tool:edit(crates/novitus/src/**), tool:write(crates/novitus/src/**)"
interruptMode: never
---

# Novitus wire encoding

Field text is Windows-1250: encode and decode it only through
`fiscal::cp1250::{encode, decode}`. Session bodies and simulator replies use
that codec while permitting protocol CR separators; receipt text fields must
pass the profile's `Capabilities::allows`, which admits no CR or other control.
UTF-8 or Latin-1 changes Polish wire text and its XOR checksum. The five
unassigned code-page bytes stay a decode error, never replacement text. ASCII
protocol identifiers and hex digits are not text fields. The driver verifies
both PC input/output code pages; discovery and fiscal operations never change
printer settings. Settings and journal JSON remain UTF-8.
