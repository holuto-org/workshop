---
description: 'Posnet wire text is Windows-1250 through posnet::text, never UTF-8 or Latin-1'
condition:
  - 'from_utf8|as_bytes\(\)|into_bytes\(\)|encoding_rs|latin'
scope: 'tool:edit(crates/posnet/src/**), tool:write(crates/posnet/src/**)'
---

Frame payloads are Windows-1250: encode and decode them only through `posnet::text::{encode, decode}`, as `frame::encode`, `frame::decode` and the simulator's replies do. UTF-8 bytes or a Latin-1 mapping corrupt Polish device text and the payload CRC, and the five unassigned code-page bytes must stay a decode error (a corrupt frame), not text. Settings and journal JSON stay UTF-8; this rule is only about bytes on the serial line.
