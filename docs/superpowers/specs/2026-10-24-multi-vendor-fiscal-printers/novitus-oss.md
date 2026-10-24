# Novitus real-world/native evidence audit

Read-only research, 2026-10-24. No printer was opened or contacted; no
repository files changed. Code was read, not copied. An offline XOR calculation
was the only executed protocol exercise. **SETTLED means the specifically stated
fact has evidence, not that all four current firmware families were
hardware-tested.**

## Sources

- **N**:
  [native NOVITUS v1.15,
  2026-02-27](https://novitus.pl/file/download/f8d4f171-162c-11f1-b940-0050569075b5),
  text alias `novitus-protocol-online-1.15.txt`; N:L refers to that extraction.
  Download the PDF as `novitus-protocol-online-1.15.pdf`, then run:

  ```sh
  pdftotext -layout novitus-protocol-online-1.15.pdf \
    novitus-protocol-online-1.15.txt
  ```

- **J**: official
  [JavaPOS driver for HD Online](https://en.novitus.pl/file/get/1239), linked by
  the [vendor driver page](https://en.novitus.pl/drivers-and-libraries) for HD
  II Online as well. Archive `novitus-javapos.zip`, SHA256
  `e20db711710f60257f622cb2fefaacdf48cb4356985c572500c174bb76a93143`;
  `javapos-1.0.jar` SHA256
  `1b5740f7a64863df116c2880cf852be80fbe61a6e98a78e571b7ea8364256bd0`.
  Documentation says version1.0h, ©2019, and explicitly names HD Online support;
  the download being linked under newer products is NOT firmware-specific
  verification. Inspected class behavior using CFR0.152; paths below are under
  `J/pl/novitus/javapos/`. No explicit
  redistribution/source license found in package; treat as proprietary, behavior
  evidence only. Download the archive as `novitus-javapos.zip`; extract with
  `unzip novitus-javapos.zip -d novitus-javapos`. Root members include
  `javapos-1.0.jar` and `javapos-driver.pdf`. Obtain
  [CFR 0.152](https://www.benf.org/other/cfr/cfr-0.152.jar) as `cfr.jar`, then:

  ```sh
  java -jar cfr.jar novitus-javapos/javapos-1.0.jar \
    --outputdir novitus-javapos/decompiled --silent true
  ```

  **J/** names `novitus-javapos/decompiled/`; Java line citations refer to
  this CFR output, not original vendor source line numbers.
- **S**: official
  [SDK
  v1.45x](https://novitus.pl/file/download/91717d03-a8ef-11f0-8af4-0050569075b5),
  linked by
  [HD II Online downloads](https://novitus.pl/wsparcie/pliki/hd-ii-online). ZIP
  SHA256 `d854b2a55f8c589190ea62cd5fef5551e5d59a03425b5c0c02c3033928591f6f`;
  `index.chm` SHA256
  `e9876c8bdb39226a52e0c1782d086767e1a7b8b6c1b8d43012401204b30a803d`. CHM
  extracted to the **S/** HTML/image namespace `novitus-sdk/help/`; contains
  vendor-published C++
  `TestLib.cpp` source in `_test_lib_8cpp_source.html`, header source, and
  photographs/scans of example receipts. Cite original Doxygen source anchors
  rather than derived text line numbers. Includes Linux1.452, macOS1.453 and
  Windows1.450 builds. Footer says ©2024 Comp SA, all rights reserved; not OSS.
  Read examples only, never run: their generic error routines include
  cancellation and are NOT Workshop-safe recovery policy. Download as
  `novitus-sdk.zip`, then use:

  ```sh
  unzip novitus-sdk.zip -d novitus-sdk
  7z x -y novitus-sdk/index.chm -onovitus-sdk/help
  ```

  `index.chm` is a root archive member; HTML/image names below are relative
  to S/. The optional derived `TestLib.cpp`, `FDTypes.h`, and `BaseLib.h`
  extracts come from `_test_lib_8cpp_source.html`,
  `_f_d_types_8h_source.html`, and `_base_lib_8h_source.html`: parse HTML
  with BeautifulSoup's `html.parser`, select `div.fragment > div.line`,
  remove each `.lineno`, collect `div.get_text().lstrip('\xa0')`, and join
  with newlines. Missing comment/blank source rows mean these derived text
  line numbers differ; citations use original Doxygen anchors instead.
- **L**:
  [litex.novitus](https://github.com/mwegrzynek/litex.novitus/tree/7cdc02037efaa49f0004d5ad8e5bb736d76b81d7),
  Python, LGPL-3.0, commit `7cdc02037efaa49f0004d5ad8e5bb736d76b81d7`
  (2020-09-11).
- **LX**:
  [litex.novitus_xml](https://github.com/mwegrzynek/litex.novitus_xml/tree/20d6dfa7d2218e861dab5ec71408c06035d3a08c),
  Python, LGPL-3.0, commit `20d6dfa7d2218e861dab5ec71408c06035d3a08c`.
- **C**:
  [SalesApp](https://github.com/MichalPajor/SalesApp/tree/44d355759023e4201a4af8ababf7ca6b4b65f987),
  C#, no license declared in repository metadata. Packet examples, not claimed
  production certification.
- **P**:
  [cpp_deon_printer](https://github.com/wojtron2/cpp_deon_printer/tree/467f4f3e60cf8442e2ec05d6aa13cbef45688ea5),
  C++, no LICENSE found/API license endpoint404. README explicitly calls it a
  showcase. Useful corroboration, not strong hardware evidence.
- **A**:
  [novitus-noviapi](https://github.com/dekoza/novitus-noviapi/tree/732dc471fa52e40aecbc3ec9ea152ed9eab8e27d),
  Python, Apache-2.0. Distinguishes vendor claims from POINT1.00 project
  hardware evidence and reports consumer observations with unknown firmware
  provenance.

For **L**, **LX**, **C**, **P**, and **A**, fetch the repository and exact
commit named by each URL above; inspect the cited paths without building or
running printer examples. A reproducible checkout uses
`git clone <repository-url> <alias>` followed by
`git -C <alias> checkout <commit>`.

- **POINT**: [official POINT manual v03][point-manual], edition 1, 2023.
  Download as `novitus-point.pdf`; run
  `pdftotext -layout novitus-point.pdf novitus-point.txt`.
  `POINT:Lx–Ly` refers to this text extract, including printed page 76.
- **USB/novitus/**: [official online x64 driver][novitus-usb],
  *Novitus Virtual COM Port driver*, DriverVer 04/28/2014,1.3.6.
  Save as `novitus-usb.zip`; run `unzip novitus-usb.zip -d novitus-usb`.
  The alias names that extraction root; member
  `drukarki_online_x64/novitus.inf` is decoded as `novitus.utf8.inf`
  beside it with `iconv -f CP1250 -t UTF-8 <input.inf> > <output.utf8.inf>`.
  Preserve blank lines. The linked [POINT ZIP][point-usb] uses the same
  member layout and decoding recipe.

[novitus-usb]: https://novitus.pl/file/download/3180ebbe-ead8-11eb-a3d6-005056904019
[point-usb]: https://point.novitus.pl/files/drukarki_online_x64.zip

[point-manual]: https://point.novitus.pl/files/Instrukcja_Obslugi_POINT_v03_20231114.pdf

## 0. Highest priority: prove the current logical PC endpoint before fiscal traffic

### Direct read-only answer: `50$i`, not `?ifi`

**SETTLED in native v1.15 and corroborated by the vendor SDK.** N §3.1.6,
L1969–1993, defines:

- Request: `ESC P 50$i ESC \\`. There is **no checksum field in this request
  table**.
- Reply: `ESC P 50$I<port-name>CR<CC>ESC \\`.
- `<port-name>` is explicitly **`PC1`, `PC2`, or `PC3`**. `CC` is the
  two-character checksum of the reply body.
- This is self-description of the communication port on the current connection;
  no index/physical-port parameter is supplied. It is the appropriate read-only
  gate to reject PC2 before sending any receipt command. The expected fiscal
  channel name is `PC1` (the GUI calls it `PC`/`1 PC`). PC3 is not established
  as fiscal-capable by this evidence and must not be assumed equivalent.

S independently exposes `NDRV_COMMUNICATION_PORT=14` and
`NDRV_READ_COMMUNICATION_PORT=15`; `CommunicationPort` holds `infoType` and
`portName`. Its published example `TestLib::CommPortInfo`, original
`TestLib.cpp:4276–4281` (`_test_lib_8cpp_source.html#l04276`), sends the query
then reads this structure. This is real SDK use of the same capability, not an
invented probe.

**`?ifi` is a false lead:** N §3.1.19, L3863–3953, queries OS network interfaces
`eth0`/`wlan0`/`ppp0` or Ethernet cable state, NOT PC/PC2. For example
`F("0;?ifi1/")`→`F("1;$ifiwlan0"+CR+"1/")` according to the field tables; vendor
worked examples omit the semicolon present in those tables. Do not use this
ambiguous network packet to establish fiscal endpoint ownership.

### Read-only configuration mapping

N §3.18.7, L13323–13347, defines read `F("^c")`; explicitly **Porty: PC1**.
Reply is
`F(decimal_option_count+"^c"+(decimal_option_number+CR+value+CR)*count)`; final
checksum/end at L15344–15345. N §3.18.8, L15761–15783, defines bounded read:

`F("0;1^c"+first_option+"/"+last_option+"/")`.

Thus `F("0;1^c42/47/")` reads configured COM1–COM6 assignments. §3.18.8 does not
independently state PC2 availability, so **do not depend on this read succeeding
on PC2 to discover that you are on PC2**; use `50$i` first.

Relevant options:

- **Option — 5**: **Meaning**: Configured dialect; **Values/source**: 0=NOVITUS
  compatible;1=NOVITUS;2=XML. N:L13380–13386. Read-only way to identify the
  configured dialect once PC1 is established.
- **Option — 42,43,44,45,46,47**: **Meaning**: COM1,COM2,COM3,COM4,COM5,COM6
  logical assignment; **Values/source**: 0=none,1=PC,2=PC2,3=PP1,4=PP2,5=print
  monitor,6=modem,7=ECREFT,8=PPP1,9=PPP2. N:L13731–13768.
- **Option — 91,92**: **Meaning**: PC input/output code page; **Values/source**:
  0=Mazovia,1=Windows1250,2=ISO8859-2,3=CP852. N:L13994–14013.
- **Option — 93,94**: **Meaning**: PC2 input/output code page;
  **Values/source**: Same page enum; **not** physical PC/PC2 routing.
  N:L14014–14033.

[GoPOS
configuration](https://help.gopos.pl/pl/articles/11864446-drukarki-fiskalne-novitus-konfiguracja)
explicitly covers HD/HD II/Deon/Bono/Point Online: native **NOVITUS**, assign
**USB1→1PC**, others0.
[FakturaLight Deon
Online](https://fakturalight.pl/fakturalight/instrukcja/novitus-deon-online)
identifies **COM3(USB1)→1PC**, or COM5(TCP)→1PC. These are deployment settings,
not proof USB1 can never be reassigned.

### PC2 is not a fiscal endpoint; exact refusal behavior remains open

- S `index.html:48–52`: PC supports the full driver; PC2 **excludes transaction
  packets**.
- [POINT manual
  v03](https://point.novitus.pl/files/Instrukcja_Obslugi_POINT_v03_20231114.pdf),
  printed p76, `POINT:L2813–L2834`, says PC
  is the main sales application and PC2 applications **cannot print receipts**;
  they get a restricted set for additional nonfiscal output, after the PC
  receipt is finished.
- [Fakturownia
  troubleshooting](https://pomoc.fakturownia.pl/2350920-Bledy-z-wydrukiem-fiskalnym-i-konfiguracji-modulu-Paragony-pl),
  section “Brak wydruku fizycznego paragonu…” explicitly reports PC2 can
  establish a connection and return the module's expected answers while the
  paper receipt is **not fiscalized**; e-receipt attempts return
  `[21] Paragon nie został rozpoczęty`. It names COM4(USB2) as an example where
  unintended PC2 should be removed.
- N global errors82/99 mean “Niedozwolony rozkaz” (L20473–20476), but there is
  **no evidence tying either universally to receipt packets arriving on PC2**.
  Do not claim an automatic82 refusal. The field report does **not** prove a
  fiscal receipt is converted into a printed nonfiscal receipt; it says no
  fiscalization. Whether individual fiscal commands are ignored, answered
  permissively, or refused remains **STILL OPEN**.

## 1. VAT sentinel contradiction: actual parsing code found

### Vendor implementation and independent parser agree

**J `data/TaxRate.java:setRate`, decompiled L33–50**, explicitly parses:

| Wire value  | Vendor class meaning                  |
| ----------- | ------------------------------------- |
| 98.99       | FREE/exempt                           |
| 99.99       | UNUSED/inactive                       |
| 100 /100.00 | FREE; normalized internally to98.99   |
| 101 /101.00 | UNUSED; normalized internally to99.99 |

J `Consts.java:8–11` independently defines FREE=`98.99`, UNUSED=`99.99`. J
`service/impl/printer/LegacyTaxRatesService.java:53–58` actually obtains rates
through **`getReport(23)`**; `LegacyPOSReportService.java:26–53` sends
`<mode>#s`, parses response `2#X`, and passes all seven returned PTU strings to
this `TaxRate` class. This is precisely code that parses Novitus PTU readback,
not a generic VAT enum.

[L
`helpers.py:46–76`](https://github.com/mwegrzynek/litex.novitus/blob/7cdc02037efaa49f0004d5ad8e5bb736d76b81d7/litex/novitus/helpers.py#L46-L76)
parses `2#X` and maps **100.00→free,101.00→unused**.
[Its captured-looking test fixture,
`tests/test_helpers.py:17–20`](https://github.com/mwegrzynek/litex.novitus/blob/7cdc02037efaa49f0004d5ad8e5bb736d76b81d7/tests/test_helpers.py#L17-L20)
contains `23.00/08.00/05.00/00.00/100.00/101.00/101.00/169/...`;
`test_taxrates_get` expects E=free (`tests/test_commands.py:275–279`). The
client requests mode22, not23 (`printer.py:149–158`). Fixture
provenance/model/protocol setting is not recorded: it cannot prove native online
firmware returns100/101.

**CONTRADICTED:** the administrative prose reversed mappings in N:L20028–20037
(native99.99 exempt/98.99 inactive; compatible101 exempt/100 inactive). Both
official JavaPOS code and independent Python parse the opposite. N cash-info and
programming tables, L1203–1205,L4769–4773, agree with official JavaPOS on98.99
exempt/99.99 inactive.

**SETTLED:** the vendor's deployed parser semantics; do not reverse them based
on administrative prose. **STILL OPEN:** actual native-mode98.99-vs100
representation returned by each current online firmware. A dialect read (option5
above) plus a provenance-labelled readback trace would settle that remaining
issue. No strong evidence found that sentinel inference alone identifies the
dialect safely.

## 2. Advance settlement: native-oriented SDK example plus actual printed receipt

The most useful find is S, beyond the already-reviewed XML example:

- `TestLib::testRCPWithSettlementPrepayment`, original `TestLib.cpp:688–725`,
  `_test_lib_8cpp_source.html#l00688`.
- Start online receipt (`numberOfLines=0`), sell **Telewizor, quantity1,
  PTUA,100000 grosz**; then
  `Prepayment{action=1,itemName="Telewizor",ptu="A",value=10000}`; then
  `NDRV_RCP_END1` with **sum90000**, cash flag1, cash100000.
- SDK overview explicitly says amounts are grosze (`index.html:86–91`). The
  example's `extra` is left zero in its zero-initialized structure.
  `index.html:795–821` reiterates `extra` is ignored; device calculates
  remainder and applies settlement to **first matching product on the receipt**.
- **Actual vendor receipt image**:
  `S/testRCPWithSettlementPrepayment.png`, distributed
  inside S `index.chm`, shows item1000.00A; literal printed **“Rozliczenie
  zaliczki na: Telewizor”**, then **−100.00A**, **“Suma do dopłaty:900.00”**,
  taxable salesA900.00, VAT23%168.29, **SUMA PLN900.00**, **DO ZAPŁATY900.00**,
  cash1000/change100. Receipt dated24-10-2019; no exact model/firmware shown.
  This is stronger evidence for **one-item partial settlement with remaining
  payment** than an unexercised schema. It does not prove all current firmware
  combinations.
- This vendor API maps the native protocol concepts very directly, but the SDK
  binary's emitted raw bytes were not captured or executed here. Native `@z`
  correspondence is supported by N §3.6.7, L8617–8651: action1 settlement,
  product nameCR, VAT letterCR, value/, extra/, first found product, reverse
  settlements before item storno. Do not claim the SDK source itself exposes a
  raw `@z` wire trace.

**Business integration evidence:**
[Mała Księgowość release
notes](https://static.presspublica.pl/red/rp/html/mala_ks/poprawki.html),
version2020.4.1: added advance settlement on fiscal receipts for **Novitus
Online and Posnet Online**. Version2020.5.1 fixes advance-invoice fiscalization
for those printers.
[SOHO change
history](https://oprogramowanie.novitus.pl/?inc=aktualizacje&news=SOHO&opc=update)
has fixes for settlement receipts containing multiple stays/advances
(v3.19,2019-11-05), and identifiers for settlement document items in
v3.19b(2019-11-26). That proves feature use, not wire allocation semantics.

**Zero total:** [vendor FAQ8](https://online.novitus.pl/najczestsze-pytania)
explicitly says choose a product already sold on the receipt, settle its
advance, and when advance equals product sale price the receipt closes at0.00.
FAQ9 permits settlement on any register of the taxpayer. **These are
cash-register UI instructions, not native printer close traces**. Zero-total
business workflow is SETTLED for Novitus online registers; native printer
`@z`+zero close remains STILL OPEN.

No inspected native OSS driver implements `@z`; the SDK's published four advance
examples are single item/single VAT. **STILL OPEN:** exact fiscal-name
canonicalization, duplicate-name targeting, multiple settlements on one item,
per-VAT aggregate allocation, deductions crossing items, full item/receipt
deduction and zero-payment native close. No basis found to substitute `$L`
discounts for legal advance settlement.

### Related `$L` evidence

S publishes `TestLib::testRCPMultipleDiscountMarkupPTU` (find by that function
in `_test_lib_8cpp_source.html`): same receipt includes A/B/C products, one
adjustment PTUNumber0 and another PTUNumber2, then close. It calls
`NDRV_RCP_DISCOUNTPTU` twice. **SETTLED at vendor-example level:** more than one
VAT bucket may be adjusted in a receipt; it is not limited to one `$L` bucket.
This is still discount/markup evidence, **not** proof those adjustments may
legally replace advances or reach zero.

## 3. Close sequences/payment forms/readback actually used

- **`$e`:** L `printer.py:472–493` sends `1;0;0;1;1;1$e` with
  cashierCR,cash/,total/,discount/. Its paper-marked test adds separate `$b`
  card+cash before `$e` (`tests/test_commands.py:226–268`). C
  `SalesApp/SalesApp/Fiscal/Packets.cs:172–185` has plain `1$e` and bundled `$y`
  whose ending is0. P `src/novitus_protocol.cpp:173–213` constructs `1;0$e` with
  cash=total. These are code-use facts, not proof of every composition's
  legality or completion timing.
- **`$y` ending3 + footer + final end:** J `LegacyReceiptService.java:275–349`
  really uses ending3, builds bundled payment types/names/amounts, sends `$y`,
  checks errors, sends footer lines using `200;0$w`, then **`28$z`** and checks
  again. It demonstrates the extra-line close lifecycle in a vendor
  implementation. This is NOT the exact proposed `$b`+`$y`
  no-bundled-payments+`12$z` path.
- **`$x`/`$y` SDK coverage:** S has separate `NDRV_RCP_END1` (payment close1)
  and END2 (payment close2); the settlement example uses END1.
  `testRCPMultiplePayments` sends separate Payment card1,cheque2,**transfer8**
  then END2 with total/payment500grosz. Library internals/raw `$x` bytes were
  not exposed, so identify this as SDK evidence rather than inventing an emitted
  frame. `testRCPMultiplePaymentsInEnd` demonstrates bundled payment use too.
- **Native `$b` IDs:**
  [L
  `printer.py:33–45,405–426`](https://github.com/mwegrzynek/litex.novitus/blob/7cdc02037efaa49f0004d5ad8e5bb736d76b81d7/litex/novitus/printer.py#L33-L45)
  explicitly sends
  cash0/card1/credit5/account6/currency7/**transfer8/mobile9**/voucher10;
  action1 records payment. S independently exemplifies transfer8 as above. No
  mobile9 printed trace found.
- **Caution on older bundled enums:** J `Consts.java:15–23` calls code6 TRANSFER
  (older account-based enum) and lacks mobile9. It cannot justify changing
  current native `$b`8/9. Enumerations are command- and generation-specific.
- **Receipt number readback in practice:** J `HDeFiscalPrinter.java:getData`,
  decompiled L1145–1164, case3 reads `getReport(23).getReceiptCount()`.
  `LegacyPOSReportService` parses the receipt-count field of `2#X`. This proves
  use, not global receipt identity. N explicitly permits daily numbering; do not
  treat mode23's count as globally unique. SDK `statusDLE2` example obtains
  `lastReceiptNr`, but its demo statement after `return` is unreachable; it is
  API-shape evidence, not exercised recovery. No OSS native `?l` recovery trace
  found.

STILL OPEN: current firmware mobile9 bundling support; exact ACK timing;
`$b`+ending3+`12$z` composition across all four targets; fully-prepaid
zero-payment close. None of these examples licenses device retries or reprints.

## 4. Error mode, DLE/DLE2

- **DLE byte layout corroborated:** L `printer.py:100–117` sends10h, takes bit2
  online,bit1 paper error,bit0 mechanism error. J
  `LegacyFiscalPrinterProtocol.java:110–124` does the same after ENQ05h.
- **ENQ polarity important:** J interprets bit2 as **success**, consistent with
  N:L393–395 (`CMD=1` command completed correctly). L exposes that same bit as
  `lastcommanderror='yes'` when set; this name is misleading/inverted relative
  to its meaning. Do not copy its field semantics blindly.
- **Mode3 NOT settled by OSS:** L defines mode3 (`autowithoutdisplay`) but
  normal examples/tests select **mode1 silent**, then sleep100ms and query`#n`
  (`printer.py:19–24,69–98,125–138`). It has no `#Z` ACK dispatcher. J
  initializes **`1#e`**, uses ENQ/DLE, and fetches`#n` only when CMD says
  failure (`LegacyFiscalPrinterProtocol.java:85–136`). S examples generally use
  mode1; `ErrorsControlMode` demonstrates2, not a raw mode3 parser.
- S exposes DLE2 structures and send/read functions, but implementation is
  binary; no complete native DLE2 trace or hex-byte-order parser was found.
  Exact DLE2 byte order, fragmented ACK ordering, initialization mode switch
  response and read-command exceptions remain **STILL OPEN**. `#n` parsers use
  full integers; official resource lists errors1000+ and9999, corroborating the
  need for more than two error digits.

## 5. Checksum example independently verified

**SETTLED.**
[L
`helpers.py:18–43`](https://github.com/mwegrzynek/litex.novitus/blob/7cdc02037efaa49f0004d5ad8e5bb736d76b81d7/litex/novitus/helpers.py#L18-L43),
[its offline
fixture](https://github.com/mwegrzynek/litex.novitus/blob/7cdc02037efaa49f0004d5ad8e5bb736d76b81d7/tests/test_helpers.py#L8-L14),
J `LegacyPacketChecksum.java:13–18`, and P `src/novitus_protocol.cpp:14–52` all
use FF XOR every payload byte, two uppercase ASCII hex digits. Independent
arithmetic here produced:

- `0#i100/`→**9B**, frame `1B 50 30 23 69 31 30 30 2F 39 42 1B 5C`.
- `210#s`→**9C**, matching N's protected-memory example.
- `23#s`→AE; `3#e`→8A if these bodies are checksummed.

No deposit command was sent; `0#i100/` is fiscal/cash-changing and is never a
probe.

## 6. Original model identity strings

**STILL OPEN** for exact native `30#v`/`#v` originals on HD II Online, Deon
Online, Bono Online and Point. J `LegacyPOSReportService.java:80–88` sends`#v`,
parses`1#R`, concatenates model/version strings without a hardcoded model table.
A unit fixture says POINT1.00, but it is NoviAPI status and not a native
identity capture. Marketing/product names and a vendor documentation example
“NOVITUS HD ONLINE” must not be promoted into all four native wire strings.

## 7. Code page practice

**SETTLED: both Mazovia and CP1250 are actually used, configuration-specific.**

- L commit
  [7cdc020](https://github.com/mwegrzynek/litex.novitus/commit/7cdc02037efaa49f0004d5ad8e5bb736d76b81d7)
  is titled “Zmiana domyślnego kodowania na Mazovię”: default changed
  CP1250→Mazovia, while its hardware fixture explicitly continues
  selectingCP1250 (`tests/conftest.py`).
- J `LegacyFiscalPrinterProtocol.java:29–38` explicitly converts outgoing
  strings to Mazovia.
- [Fakturownia
  troubleshooting](https://pomoc.fakturownia.pl/2350920-Bledy-z-wydrukiem-fiskalnym-i-konfiguracji-modulu-Paragony-pl),
  Polish-character/error16 section, instructs Mazovia(CP790).
- [BigDot PosnetServer
  setup](https://blog.bigdotsoftware.pl/posnet-server-konfiguracja-drukarki-fiskalnej/)
  explicitly gives Novitus input “Mazowia”, output “Windows1250”,
  USB1(COM3),115200. Asymmetry is real deployment practice; do not assume one
  codec both directions.
- Native read-only options91–94 above make this inspectable. Full printable
  repertoire/undefined-byte/font behavior remains unverified.

## 8. Company name/footer practice

J provides a concrete end3→footer→end path:
`LegacyReceiptService.java:234–235,275–349,477–547`, message-type24 free text;
its `messages_en.xml` has an **empty** prefix for `FPTR_MT_FREE_TEXT`, so this
route is label-free at SDK level. It emits `200;0$w` lines and final`28$z`. The
formatter is called with width40 but **does not enforce that argument**, merely
concatenating prefix+text: not proof of a40-byte device limit. J
`HDeFiscalPrinter.setTrailerLine` is unsupported despite appearing in its
feature list.

[Vendor FAQ23](https://online.novitus.pl/najczestsze-pytania) requires taxpayer
name/postcode/town in fiscal header; headquarters details may be configured as
other information. Neither that nor JavaPOS establishes legality or persistence
behavior for arbitrarily changing per-job CompanyName. Daily declaration
restrictions, `@ws` attachment/PC2 queue behavior and each current model's
supported widths remain **STILL OPEN**.

## 9. USB CDC/Linux and A/B

- Official [online Windows INF](https://en.novitus.pl/file/get/1238),
  `drukarki_online_x64/novitus.inf`, local UTF8 copy
  `USB/novitus/drukarki_online_x64/novitus.utf8.inf:27–32,84–85`:
  **A=VID1A28/PID2512/MI_00;B=MI_02**. The
  [Point package](https://point.novitus.pl/files/drukarki_online_x64.zip) uses
  the same mapping.
- Actual integrations consistently configure **USB1 for fiscal PC**; Fakturownia
  explicitly mentions USB2(COM4) wrongly assignedPC2. **A↔USB1 is a likely
  mapping, not directly proved by a descriptor+configuration trace**. Never
  identify the fiscal endpoint from A/B or the smallest tty number alone; `50$i`
  answers the logical question directly.
- J documentation p6 enumerates **ttyACM0 and ttyACM1**, both “Novitus Virtual
  COM port”, default9600/8N1/xonxoff; this is vendor Linux use evidence for two
  ACM ports. It does not establish stable enumeration order.
- BigDot gives `/dev/ttyACM0,115200,8,N,1,N` as typical Linux Novitus/Posnet
  setup. Its **shown `cdc_acm`/udevadm trace is explicitly PosnetVID1424**, NOT
  Novitus; do not cite that trace as a Novitus descriptor capture. No direct
  Novitus Linux descriptor/dmesg trace found. CDC baud/modem-line effects and
  robust reopen behavior remain open.

## XML/NoviAPI reliability: evidence, no protocol selection decision

There is **no evidenced community consensus that either is more reliable for the
unsettled advance/mobile/zero-close cases**.

-
  [FakturaLight](https://fakturalight.pl/fakturalight/instrukcja/novitus-deon-online),
  [Warsztat24](https://warsztat24.com/konfiguracja-drukarek-fiskalnych-novitus-usb-novitus-remote-printer/)
  and
  [Streamsoft7.0.347](https://portalklienta.streamsoft.pl/co-nowego/wersja-7-0-347/)
  document real **XML** deployment. Streamsoft explicitly says no extra
  libraries, COM orTCP/IP, but that release excludes invoices/payment-method
  coverage. None offers comparative failure data or native-vsXML advance traces.
- **Native is also current real-world practice:** GoPOS explicitly requires
  nativeNOVITUS for all target online families; Fakturownia supports
  native/compatible and says active NoviAPI blocks native PC sales with
  error1528. This is mutual-mode behavior, not a reliability ranking.
- [Directo's integration guide](https://wiki.directo.ee/et/novitus_fp_config)
  uses NoviAPI HTTPS.
  [A hardware
  guide](https://github.com/dekoza/novitus-noviapi/blob/732dc471fa52e40aecbc3ec9ea152ed9eab8e27d/docs/hardware-testing.md#L20-L52)
  reports only POINT1.00 personally verified; other current model minimum
  versions are vendor declarations.
- [A lifecycle
  research](https://github.com/dekoza/novitus-noviapi/blob/732dc471fa52e40aecbc3ec9ea152ed9eab8e27d/docs/research/request-lifecycle.md#L62-L107)
  reports a consumer test where no-paper confirm succeeds quickly, confirmed job
  remainsPENDING, prints when paper returns, and DELETE of a confirmed job is
  refused then it prints15min later. Model/firmware of that consumer device is
  **unspecified**; these are published second-order field observations, not our
  measurements. Its own POINT tests are status/nonfiscal; fiscal advance
  reliability is not established.
- A records protocol/schema disagreements, queue pruning on daily reports, and
  token/concurrency hazards. The vendor's send/confirm/status design offers
  different reconciliation tools, but it is not a blanket exactly-once or
  safe-retry guarantee. The NoviAPI example `receipt_with_advance_and_container`
  **adds** an advance (10+5=15); it is not a settlement/zero-total example.

## Gap/status table

- **Gap — 0 current connection PC vsPC2**: **Status**: **SETTLED**; **What is
  actually settled / what remains**:
  Read-only`50$i`→`50$I`+PC1/PC2/PC3CR+checksum; vendor SDK also uses it.
- **Gap — 0 physical/config mapping**: **Status**: **SETTLED**, partial; **What
  is actually settled / what remains**: Config42–47 mapsCOM1–6 toPC/PC2;
  integrationCOM3=USB1. Exact all-model A↔USB1/USB2 mapping **STILL OPEN**.
- **Gap — 0 PC2 receipt refusal**: **Status**: **STILL OPEN**; **What is
  actually settled / what remains**: Cannot fiscalize is settled; expected
  answers/no fiscalization field report; no universal refusal code established.
- **Gap — 1 VAT decimal sentinel meanings**: **Status**: **CONTRADICTED**
  administrative prose; **What is actually settled / what remains**: Vendor
  implementation98.99 exempt/99.99 inactive; matches command tables.
- **Gap — 1 VAT100/101 semantics**: **Status**: **SETTLED** parser behavior;
  **What is actually settled / what remains**: Vendor+OSS100 exempt/101
  inactive; reversed admin prose contradicted.
- **Gap — 1 actual native-mode readback on each current firmware**: **Status**:
  **STILL OPEN**; **What is actually settled / what remains**: No
  provenance-labelled current online native trace; option5 gives read-only
  dialect check.
- **Gap — 2 single-item partial advance and remaining payment**: **Status**:
  **SETTLED** vendor example/image; **What is actually settled / what remains**:
  1000A−100A=900; taxable900; cash1000/change100. No raw SDK packet capture.
- **Gap — 2 multiple items/rates, duplicates, canonicalization**: **Status**:
  **STILL OPEN**; **What is actually settled / what remains**: First found item
  documented; no sufficiently specific multi-rate native integration source.
- **Gap — 2 fully-prepaid zero native receipt**: **Status**: **STILL OPEN**;
  **What is actually settled / what remains**: Vendor FAQ confirms0.00
  cash-register workflow, not native printer close bytes.
- **Gap — 3 close practices**: **Status**: **SETTLED**, partial; **What is
  actually settled / what remains**: OSS`$e`; vendor`$y`ending3→footer→28$z; SDK
  payment-close1/2; exact desired composed path/timing **STILL OPEN**.
- **Gap — 3 payment5/8/9**: **Status**: **SETTLED** code enums; trace open;
  **What is actually settled / what remains**: OSS`$b`credit5/transfer8/mobile9;
  SDK transfer8; mobile printed/current bundled trace absent.
- **Gap — 3 receipt number readback**: **Status**: **SETTLED** use; durability
  open; **What is actually settled / what remains**: Vendor uses23#s receipt
  count and exposesDLE2 lastReceiptNr; not unique recovery proof.
- **Gap — 4 DLE bits/ENQ CMD**: **Status**: **SETTLED**; **What is actually
  settled / what remains**: DLE low3 bits agreed; CMD=success. OSS
  error-labelled field is misleading.
- **Gap — 4 mode3 ACK order and DLE2 byte order**: **Status**: **STILL OPEN**;
  **What is actually settled / what remains**: Inspected implementations
  chieflymode1; no rawmode3/DLE2 parser proof.
- **Gap — 5 checksum example**: **Status**: **SETTLED**; **What is actually
  settled / what remains**: Independent arithmetic+three implementations+fixture
  agree9B;210#s9C also verified offline.
- **Gap — 6 original model identity strings**: **Status**: **STILL OPEN**;
  **What is actually settled / what remains**: No native30#v/#v originals for
  all four target families.
- **Gap — 7 encoding practice**: **Status**: **SETTLED**, repertoire open;
  **What is actually settled / what remains**: BothMazovia/CP1250 used;
  asymmetricin/out configurable/readable.
- **Gap — 8 label-free footer mechanism**: **Status**: **SETTLED** old vendor
  implementation; **What is actually settled / what remains**:
  end3+200;0$w+28$z; current widths/dailyCompanyName restrictions **STILL
  OPEN**.
- **Gap — 9 A/B hardware interface numbers**: **Status**: **SETTLED**; **What is
  actually settled / what remains**: VendorINF A=MI00/B=MI02.
- **Gap — 9 LinuxCDC lifecycle/order**: **Status**: **STILL OPEN**, partly
  corroborated; **What is actually settled / what remains**: Vendor enumerates
  twoNovitusttyACM ports; no stable order/modem-line/reopen proof.
- **Gap — Original audit#5 ownership/exclusion**: **Status**: **STILL OPEN**;
  **What is actually settled / what remains**: Self-identification is not an
  all-interface exclusive lock; no new native ownership guarantee.
- **Gap — Original audit#7 multi-PTU`$L`**: **Status**: **SETTLED** vendor
  example only; **What is actually settled / what remains**: TwoVAT bucket
  adjustments shown; legality as advances/zero limits remain **STILL OPEN**.
- **Gap — Original audit#9 long names/count limits**: **Status**: **STILL
  OPEN**; **What is actually settled / what remains**: No new60-byte/?l40-byte
  or>255-line field evidence.
- **Gap — Original audit#11 clock epoch/DST**: **Status**: **STILL OPEN**;
  **What is actually settled / what remains**: No new device-level evidence.
- **Gap — Original audit#13 monthly-report edge cases**: **Status**: **STILL
  OPEN**; **What is actually settled / what remains**: No new field
  trace/refusal-timing evidence.
- **Gap — Original audit#14 reconciliation durability**: **Status**: **STILL
  OPEN**; **What is actually settled / what remains**: Readback API use is not
  snapshot/reboot/system-ID uniqueness guarantee.
- **Gap — XML/NoviAPI more reliable than native**: **Status**: **STILL OPEN**
  comparative claim; **What is actually settled / what remains**: Adoption
  evidence on all sides, no comparative feature-specific reliability
  study/consensus found.
