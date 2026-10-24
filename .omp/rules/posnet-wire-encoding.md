---
description: 'Use CP1250 for Posnet wire frames in both driver and simulator'
astCondition:
  - '$X.encode()'
  - '$X.decode()'
condition:
  - '\.(?:encode|decode)\s*\(\s*[''"](?:utf-?8|ascii|latin-?1|iso-8859-1)[''"]'
scope: 'tool:edit(packages/posnet/posnet/protocol.py), tool:write(packages/posnet/posnet/protocol.py), tool:edit(packages/posnet/posnet/simulator.py), tool:write(packages/posnet/posnet/simulator.py)'
---

Encode and decode Posnet wire payloads explicitly with `cp1250`, as `protocol.encode_frame`, `protocol.decode_frame`, and `Simulator._respond` do. Default UTF-8 or an ASCII/Latin-1 codec corrupts Polish device text and the payload CRC. This is the serial-frame boundary, not JSON/settings storage (which remains UTF-8).
