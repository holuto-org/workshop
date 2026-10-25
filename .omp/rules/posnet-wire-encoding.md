---
description:
  "Posnet wire text is Windows-1250 through fiscal::cp1250, never UTF-8 or Latin-1"
condition:
  - 'from_utf8|as_bytes\(\)|into_bytes\(\)|encoding_rs|latin'
scope: "tool:edit(crates/posnet/src/**), tool:write(crates/posnet/src/**), tool:edit(crates/fiscal/src/cp1250*), tool:write(crates/fiscal/src/cp1250*)"
interruptMode: never
---

# Posnet wire encoding

Frame payloads are Windows-1250: encode and decode them only through
`fiscal::cp1250::{encode, decode}` via `posnet::text`, as `frame::encode`,
`frame::decode` and the simulator's replies do. UTF-8 bytes or a Latin-1 mapping
change the intended Polish wire text and its CRC; the five unassigned code-page
bytes stay a decode error (a corrupt frame), never replacement text. Settings
and journal JSON stay UTF-8; this rule concerns only bytes on the serial line.
