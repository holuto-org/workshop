---
description: "Native ElzabSTX framing, configured code page and fiscal completion"
condition: ".*"
scope: "tool:read(crates/elzab/**), tool:edit(crates/elzab/**), tool:write(crates/elzab/**)"
interruptMode: never
---

# ElzabSTX

Read the `crates/elzab` module docs before changing native bytes or completion
semantics.

- Frame CRC covers header, FS and data; CRC-16/ARC, four uppercase hex digits.
  Requests use fresh decimal IDs. Match reply IDs, validate transport status and
  CRC before command errors; replies do not echo command names. Never resend any
  frame, including on SL, silence, bad CRC or disconnect.
  Apply documented empty-header defaults before matching (ID/frame/max 1,
  status SA, transform NDT). Multipart frame numbers must be sequential; a
  declared nonzero maximum cannot change. Unknown maxima remain 0 until the
  final frame declares its own number.
- Require read-only `XHR GS w` to report CP1250. Use `fiscal::cp1250`; printable
  repertoire excludes controls and undefined bytes. Reject text separators and
  unrepresentable input without replacement, truncation or transliteration.
  Quantities have at most five decimals; comma is the native decimal separator.
- Exact XTR model names qualify candidate ONLINE profiles, not legacy numeric
  IDs or marketing substrings. Model/firmware hardware qualification remains
  open. The 255-item cap is a conservative host policy, not a measured buffer.
- Read XTR/FNR, online lowercase ySR packages (128/128/32 bits), complete company
  header, VAT and clock in the same connection before operations; inspect LPC's
  nested error before LOR. No setters or configuration changes are permitted.
- Journal every fiscal command before sending. Only a valid matched transport
  rejection of the first single-frame LOR is certain; all later failures are
  uncertain. Paper-out may auto-cancel an ONLINE receipt: never cancel, resume,
  replay or reprint automatically. Human resolution never sends commands.
- Advances use ONLINE LDS with final bare RS D. Zero-total receipts stay blocked
  before LOR. Never substitute 0.01 or a negative sale. Company LAL remains
  disabled until native ordering/completion is qualified in the profile.
- Daily RRP success is acceptance only: always uncertain. Monthly/periodic RPP
  uses S0 (synchronous). Never infer daily completion from idle/counter changes.
- Exercise only the feature-gated offline simulator, never real fiscal hardware.
