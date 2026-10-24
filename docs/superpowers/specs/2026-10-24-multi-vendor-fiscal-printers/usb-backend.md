# USB identification and backend vendor footprint (read-only research)

## Sources

`USB/` is a citation namespace for the files listed here, not a repository
path. Line citations address the extracted text or decoded INF, not PDF
pages. Sources were collected on 2026-10-24; web pages and mutable downloads
can change, so their current line numbers need not match this snapshot.
Download files under the alias filenames. For PDFs, use
`pdftotext -layout <pdf> <txt>` with the same stem. For ZIPs, use
`unzip <zip> -d <directory>` as mapped below; inspect files only, do not run
installers. Decode cited `.utf8.inf` copies using
`iconv -f CP1250 -t UTF-8 <original.inf> > <copy.utf8.inf>`;
retain original INF files and their line endings/blank lines.

- **USB/posnet.zip**:
  [Posnet CDC v.023015][posnet-cdc], INF DriverVer 06/22/2018,2.23.15.
  Extract to `USB/posnet/`; member `posnetCDC15v.inf`, decoded as
  `posnetCDC15v.utf8.inf`.
- **USB/novitus.zip**:
  [Novitus online x64 driver][novitus-cdc], *Novitus Virtual COM Port driver*,
  DriverVer 04/28/2014,1.3.6. Extract to `USB/novitus/`; member
  `drukarki_online_x64/novitus.inf`, decoded as `novitus.utf8.inf` beside it.
- **USB/point.zip**:
  [POINT online x64 driver][point-cdc], same INF version as Novitus above.
  Extract to `USB/point/`; member `drukarki_online_x64/novitus.inf`, decoded
  as `novitus.utf8.inf` beside it.
- **USB/elzab.zip**:
  [ELZAB CDC driver][elzab-cdc], *Communication Device Class Virtual Serial
  Port*, HCC Embedded, DriverVer 08/04/2010,2.0.0.0.
  Extract to `USB/elzab/`; member `usb_cdc_ser.inf`; nested archive
  `atm6124_cdc_signed.zip` contains
  `atm6124_cdc_signed/atm6124_cdc.inf`, *Windows USB CDC Driver Setup File for
  ATMEL AT91SAM products*, DriverVer 03/09/2011,2.0.0.0.
  Extract that nested ZIP with `USB/elzab/` as the destination.
- **USB/elzab-usbk10.zip**:
  [ELZAB legacy Windows/Android USB package][elzab-legacy].
  Extract to `USB/elzab-usbk10/`; member paths cited below are relative
  to that directory, including `ANDROID/src/main/java/` and `WINDOWS/`.
  No package version is asserted.

[posnet-cdc]: https://www.posnet.com.pl/files/download/pos-h-dev-23-015-sterownik-usb-posnetcdc.zip
[novitus-cdc]: https://novitus.pl/file/download/3180ebbe-ead8-11eb-a3d6-005056904019
[point-cdc]: https://point.novitus.pl/files/drukarki_online_x64.zip
[elzab-cdc]: https://www.elzab.com.pl/download/program/Usb_cdc_ser.zip
[elzab-legacy]: https://www.elzab.com.pl/ftp/uzyt/usbk10.zip

- **USB/elzab-install.txt**: [Poradnik instalacyjny sterownika CDC-ACM dla
  systemów Windows][elzab-install-pdf], version 1.01; for CDC-ACM driver 1.0 or
  newer; downloaded as `USB/elzab-install.pdf`, extracted as
  `USB/elzab-install.txt`; no archive.

[elzab-install-pdf]: https://www.elzab.com.pl/download/program/Instalacja_sterownika_usb.pdf

- **USB/elzab-zeta.txt**: [Rozszerzona instrukcja obsługi drukarki ELZAB Zeta
  online][elzab-zeta-pdf], version 2.1; downloaded as `USB/elzab-zeta.pdf`,
  extracted as `USB/elzab-zeta.txt`; no archive.

[elzab-zeta-pdf]: https://www.elzab.com.pl/download/io/rozszerzona_instrukcja_obslugi_elzab_zeta_online.pdf

- **USB/elzab-mera.txt**: [Instrukcja obsługi drukarki ELZAB Mera
  online][elzab-mera-pdf], version 2.9; downloaded as `USB/elzab-mera.pdf`,
  extracted as `USB/elzab-mera.txt`; no archive.

[elzab-mera-pdf]: https://www.elzab.com.pl/download/io/rozszerzona_instrukcja_obslugi_elzab_mera_online.pdf

- **USB/elzab-cube.txt**: [Rozszerzona instrukcja obsługi drukarki ELZAB Cube
  online][elzab-cube-pdf], version 2.4; downloaded as `USB/elzab-cube.pdf`,
  extracted as `USB/elzab-cube.txt`; no archive.

[elzab-cube-pdf]: https://www.elzab.com.pl/download/io/rozszerzona_instrukcja_obslugi_elzab_cube_online.pdf

- **USB/elzab-t10.txt**: [Instrukcja obsługi drukarki ELZAB T10
  online][elzab-t10-pdf], version 1.2; downloaded as `USB/elzab-t10.pdf`,
  extracted as `USB/elzab-t10.txt`; no archive.

[elzab-t10-pdf]: https://www.elzab.com.pl/download/io/instrukcja_obslugi_drukarki_elzab_t10_online.pdf

- **USB/novitus-deon.txt**: [Instrukcja obsługi DEON ONLINE][novitus-deon-pdf],
  version v06 (20220928), edition I, 2022; downloaded as `USB/novitus-deon.pdf`,
  extracted as `USB/novitus-deon.txt`; no archive.

[novitus-deon-pdf]: https://novitus.pl/file/download/6c00d097-4e0c-11ed-b3b1-005056904019

- **USB/novitus-point.txt**: [Instrukcja obsługi drukarki fiskalnej
  POINT][novitus-point-pdf], version v03 (20231114), edition 1, 2023; downloaded
  as `USB/novitus-point.pdf`, extracted as `USB/novitus-point.txt`; no archive.

[novitus-point-pdf]: https://point.novitus.pl/files/Instrukcja_Obslugi_POINT_v03_20231114.pdf

- **USB/linux-usb.ids**: [linux-usb.org
  registry](http://www.linux-usb.org/usb.ids),
  version 2026.06.26 (lines 12-13); save directly, no conversion.
- **USB/usb.ids**: [maintainer HTTPS mirror](https://usb-ids.gowdy.us/usb.ids),
  second copy of that registry; save directly, no conversion.
- **USB/*-page.txt**: vendor HTML snapshots listed below (unversioned).
  Fetch UTF-8 HTML with User-Agent `Mozilla/5.0`; parse using Python's
  `html.parser.HTMLParser`. Suppress data inside `script` and `style`.
  For each other nonempty data node, append `' '.join(data.split())`.
  Write `'Source: ' + url + '\n' + '\n'.join(parts) + '\n'` as UTF-8.
  Preserve node order; the first line records the source URL.

- **USB/point-downloads-page.txt**: [Do pobrania - Drukarka fiskalna Novitus
  POINT][point-downloads-page].

[point-downloads-page]: https://point.novitus.pl/do-pobrania

- **USB/novitus-bono-page.txt**: [Drukarka fiskalna Bono Online – nowoczesne
  rozwiązanie – Novitus][novitus-bono-page].

[novitus-bono-page]: https://novitus.pl/bono-online

- **USB/novitus-hd2-page.txt**: [Drukarka Fiskalna Novitus HD II
  Online][novitus-hd2-page].

[novitus-hd2-page]: https://novitus.pl/hd-ii-online-zintegrowany

- **USB/elzab-adapter-page.txt**: [Konwerter USB-RS232 -
  ELZAB][elzab-adapter-page].

[elzab-adapter-page]: https://www.elzab.com.pl/pl/akcesoria-i-dodatki/integracja/konwertery/konwerter-usb-rs232

- **USB/elzab-programs-page.txt**: [Programy serwisowe, użytkownika i inne -
  ELZAB][elzab-programs-page].

[elzab-programs-page]: https://www.elzab.com.pl/pl/strefa-dla-klienta/do-pobrania/programy

- **USB/posnet-ergo-page.txt**: [ERGO ONLINE - Small/mobile cash registers -
  Posnet Polska S.A.][posnet-ergo-page].

[posnet-ergo-page]: https://www.posnet.com.pl/en/produkty/product/356/ergo-online

- **USB/posnet-thermal-page.txt**: [THERMAL HD ONLINE - store fiscal printers
  (commerce - services) - Posnet Polska S.A.][posnet-thermal-page].

[posnet-thermal-page]: https://www.posnet.com.pl/en/produkty/product/371/thermal-hd-online

- **USB/posnet-temo-page.txt**: [TEMO ONLINE - store fiscal printers (commerce -
  services) - Posnet Polska S.A.][posnet-temo-page].

[posnet-temo-page]: https://www.posnet.com.pl/en/produkty/product/361/temo-online

- **USB/posnet-drivers-page.txt**: [Sterowniki USB - Urządzenia fiskalne -
  Programy użytkowe, sterowniki i biblioteki - Dla klienta - Posnet Polska
  S.A.][posnet-drivers-page].

[posnet-drivers-page]: https://www.posnet.com.pl/sterowniki-usb

## Reading conventions and provenance

- Workshop root = `/home/g4m3r/Documents/HuggingCar/workshop`; backend root =
  sibling `backend`; web root = sibling `frontend`. Repo citations below are
  relative to the named root. USB evidence files use the
  `USB/` aliases and extraction recipes in Sources. No repository files were
  changed; no printer was contacted; no checks/builds were run.
- **Important distinction:** INF manufacturer/device-description strings
  describe Windows driver properties; they do **not** prove actual USB
  descriptor strings on Linux/macOS. Statements explicitly labelled
  `[UNVERIFIED]` are not established by the collected sources. Recommendations
  are labelled `[INFERENCE]`.

## A. USB brand identification

### A1. Findings that change the feasibility/scope

1. **Metadata-only filtering cannot both exclude every non-printer and preserve
   every serial printer connection.** ELZAB explicitly markets a universal
   FTDI232RL USB–RS232 adapter for printers and other devices
   (`USB/elzab-adapter-page.txt:73-78`,
   [vendor](https://www.elzab.com.pl/pl/akcesoria-i-dodatki/integracja/konwertery/konwerter-usb-rs232));
   Zeta/Cube/Mera ONLINE manuals explicitly instruct connecting a PC DB9 serial
   port to the printer RJ45 using the vendor cable
   (`USB/elzab-zeta.txt:570-581`, `elzab-cube.txt:568-586`,
   `elzab-mera.txt:556-563`). `[INFERENCE]` The host sees the adapter's USB
   descriptors or its own UART, not the printer behind RS232. Such a port can be
   valid even if its VID is FTDI/Prolific/CH340/Silicon Labs or its type is
   PCI/Unknown. Generic adapter allowlists necessarily include non-printers;
   rejecting them loses valid printer setups.
2. **Even direct USB is not an exact printer/model proof.** The ELZAB online
   driver matches CDC class/subclass/protocol, not a VID/PID
   (`USB/elzab/usb_cdc_ser.inf:22-32`); ELZAB's separate Zeta/D10 sample
   explicitly uses generic STM `0483:5740`
   (`USB/elzab-usbk10/ANDROID/src/main/java/com/hoho/android/usbserial/driver/UsbId.java:52-53`).
   `usb.ids` identifies that pair merely as STMicroelectronics Virtual COM Port
   (`USB/linux-usb.ids:3531,3569`). `[INFERENCE]` Neither the generic CDC class
   nor that pair uniquely proves ELZAB.
3. **ERGO ONLINE is a cash register, not a fiscal printer.** Posnet categorizes
   it as “Small/mobile cash registers” and calls it a cash register
   (`USB/posnet-ergo-page.txt:2,75`); its USB driver exists, but native Posnet
   **printer** protocol support for ERGO is `[UNVERIFIED]`. Its optional RS232
   modules are documented (`:89-91,188-193`). Do not equate a Posnet VID with a
   supported printer.
4. **D10 and Zeta without ONLINE are legacy devices in ELZAB's catalog.**
   Current ONLINE printer section lists T10, Mera, Cube, Zeta
   (`USB/elzab-programs-page.txt:122-127`); plain Zeta and D10 are in a separate
   non-ONLINE section (`:237-252`). A “D10 ONLINE” model was not established
   `[UNVERIFIED]`.
5. **T10 ONLINE cannot presently be promised as USB CDC serial.** Its own manual
   documents microUSB **RNDIS / virtual TCPIP**, to the dedicated D2 mini POS
   System, and lists only that communication connector
   (`USB/elzab-t10.txt:353-365,1368-1370`,
   [manual](https://www.elzab.com.pl/download/io/instrukcja_obslugi_drukarki_elzab_t10_online.pdf)).
   A serial CDC or RS232 mode is `[UNVERIFIED]`, despite the shared
   download-page grouping under CDC (`USB/elzab-programs-page.txt:124-132`). The
   model-specific manual is stronger evidence than a grouped download heading.

### A2. Posnet: own VID, not a generic bridge VID for the supplied online CDC

driver

Primary online-driver source:
[Posnet USB drivers](https://www.posnet.com.pl/sterowniki-usb), snapshot
`USB/posnet-drivers-page.txt:115-116`, explicitly names ONLINE TRIO, Ergo,
Thermal HD, Thermal XL2, Temo, Mobile, Neo. The INF uses `usbser.sys`
(`USB/posnet/posnetCDC15v.utf8.inf:24-29,59-66`) and **VID 1424** (`:72-102`),
matching Posnet Polska S.A. in `USB/linux-usb.ids:17989`. This is a Posnet VID,
not `0403/067B/1A86/10C4/04D8/0483`.

- **Device/family named by driver — Temo HS (legacy family)**: **VID:PID /
  interface**: `1424:1015`; **Windows manufacturer / description**: `POSNET
  S.A.` / `POSNET Temo HS Serial Port`; **Evidence and certainty**: INF
  `:73,89,109,111`; usb.ids `:18009`. Not proof that Temo **ONLINE** uses 1015.
- **Device/family named by driver — Thermal HD family**: **VID:PID /
  interface**: `1424:100A`; **Windows manufacturer / description**: `POSNET
  S.A.` / `POSNET Thermal HD Serial Port`; **Evidence and certainty**: INF
  `:77,93,109,115`; usb.ids `:17999`. Exact ONLINE firmware/hardware revision
  assignment `[UNVERIFIED]`.
- **Device/family named by driver — Ergo family**: **VID:PID / interface**:
  `1424:100D`; **Windows manufacturer / description**: `POSNET S.A.` / `POSNET
  Ergo Serial Port`; **Evidence and certainty**: INF `:80,96,109,118`; usb.ids
  `:18002`. It is a cash register; not printer-model proof.
- **Device/family named by driver — Generic newer Posnet cash-register
  interface**: **VID:PID / interface**: `1424:10A0`, `REV_0300&MI_00` or
  `REV_0200`; **Windows manufacturer / description**: `POSNET S.A.` / `Posnet K
  Serial Port`; **Evidence and certainty**: INF `:83-84,99-100,109,121`.
  Individual model mapping `[UNVERIFIED]`.
- **Device/family named by driver — Generic newer Posnet printer interface**:
  **VID:PID / interface**: `1424:10B0`, `REV_0300&MI_00` or `REV_0200`;
  **Windows manufacturer / description**: `POSNET S.A.` / `Posnet D Serial
  Port`; **Evidence and certainty**: INF `:85-86,101-102,109,122`. Individual
  Temo ONLINE / Thermal HD ONLINE mapping `[UNVERIFIED]`; the INF is shared, not
  per-model.

The same INF also covers `1016/1017/1018`, `100B/100C/100E/100F`
(`:74-82,90-98`). `[INFERENCE]` Broad `vid == 1424` filtering finds a vendor
family, not only online printers. Conversely, using only `1424:1001` from
usb.ids is wrong: that entry is plain “Temo” (`USB/linux-usb.ids:17990`), while
the serial-driver entries above and 10B0 exist independently.

Actual observed Linux descriptors for **Thermal XL2** (not one of the requested
three models): community dmesg shows `1424:10b0`, product `Posnet D`,
manufacturer `Posnet`, serial `4076F963`, `cdc_acm`:
[Debian user report](https://forum.dug.net.pl/viewtopic.php?id=31221). This
corroborates generic Posnet D naming but does not prove requested-model strings.
For **Temo ONLINE, Thermal HD ONLINE, Ergo ONLINE**, exact on-device
manufacturer/product strings and exact PID per hardware revision remain
`[UNVERIFIED]`. Windows INF descriptions must not be relabelled USB descriptors.

Posnet's separate FTDI driver targets older Thermal/Temo/etc. without ONLINE;
the page lists that separately (`USB/posnet-drivers-page.txt:124-128`). It also
offers a USB–RS232 converter for fiscal terminals (`:112-113`). Therefore older
Posnet FTDI presence is not evidence that current online native USB needs
generic FTDI matching.

### A3. Novitus: own VID and a shared two-interface CDC PID

- The exact **same online x64 driver link** is supplied on
  [HD II ONLINE](https://novitus.pl/wsparcie/pliki/hd-ii-online),
  [DEON ONLINE](https://novitus.pl/wsparcie/pliki/deon-online), and
  [BONO ONLINE](https://novitus.pl/wsparcie/pliki/bono-online) support pages.
  POINT supplies its own download [page](https://point.novitus.pl/do-pobrania)
  (`USB/point-downloads-page.txt:19-22`) and
  [ZIP](https://point.novitus.pl/files/drukarki_online_x64.zip).
- Both downloaded INFs are identical in the relevant content:
  **`USB\VID_1A28&PID_2512&MI_00` and `...&MI_02`**, representing virtual serial
  ports A/B (`USB/novitus/drukarki_online_x64/novitus.utf8.inf:26-32`;
  `USB/point/drukarki_online_x64/novitus.utf8.inf:26-32`). `usbser.sys` is used
  (`novitus.utf8.inf:45-69`). This is a Novitus VID, not one of the generic
  bridge VIDs; the
  [USB ID vendor registry](https://the-sz.com/products/usbid/index.php?v=0x1A28)
  calls 1A28 NOVITUS SA. The downloaded linux-usb list contains no 1A28/Novitus
  entry (literal search of the full snapshot); absence is not evidence against
  the vendor INF.
- Windows driver properties: manufacturer `Novitus`; provider
  `NOVITUS - Centrum Technologii Sprzedaży`; descriptions exactly
  `Novitus Virtual serial Port A` and `Novitus virtual serial Port B`
  (`novitus.utf8.inf:81-86`; POINT same `:81-86`). Do not lowercase or normalize
  these into purported USB descriptor strings.
- **Per-model conclusion:** HD II ONLINE, DEON ONLINE, BONO ONLINE, POINT each
  have vendor-published driver association to **1A28:2512**. There are no
  distinct model PIDs in those packages. Actual USB iManufacturer/iProduct
  strings per model and USB serial-number formats remain `[UNVERIFIED]`; the
  driver package alone cannot establish them.
- POINT manual explicitly says USB-B appears as **two standard serial ports**,
  with RJ45 COM1 as its separate RS232 connector
  (`USB/novitus-point.txt:765-783`). `[INFERENCE]` Both interfaces can be
  discovery candidates, but which is configured as fiscal PC vs PC2 must be
  established via device protocol/configuration, not by assuming every interface
  is interchangeable. The workspace currently has no `usbportinfo-interface`
  feature, so its `UsbPortInfo` cannot expose `interface` without enabling it
  (dependency feature definition in serialport 4.10.1 `src/lib.rs:831-838`;
  workspace Cargo.toml:135).

### A4. ELZAB: online package does not establish normal-operation model VID/PID

#### What is proved

1. The vendor links **Usb_cdc_ser.zip** for T10/Mera/Cube/Zeta ONLINE
   (`USB/elzab-programs-page.txt:122-132`,
   [source](https://www.elzab.com.pl/pl/strefa-dla-klienta/do-pobrania/programy)).
   The principal INF matches **`USB\Class_02&SubClass_02&Prot_01` and
   `...&Prot_00`** for x86/ia64/amd64; no VID/PID. Windows properties are
   provider `HCC Embedded`, description `CDC Device`, service
   `USB Serial Driver` (`USB/elzab/usb_cdc_ser.inf:22-32,118-121`). Its service
   is `usbser.sys` (`:45-60`). **These strings do not distinguish an ELZAB
   printer from other CDC devices.**
2. Nested `atm6124_cdc_signed.zip` matches **03EB:6124**, with Windows strings
   `ATMEL Corp.` / `AT91 USB to Serial Converter`
   (`USB/elzab/atm6124_cdc_signed/atm6124_cdc.inf:88-100`). linux-usb calls this
   **at91sam SAMBA bootloader** (`USB/linux-usb.ids:105,179`). The package does
   not map it to a model or say it is the normal printer interface. Treating
   this as “all ELZAB online printers are 03EB:6124” would be unsupported
   `[UNVERIFIED]`.
3. Legacy **Zeta/D10** support section links `usbk10.zip`
   (`USB/elzab-programs-page.txt:237-252`). Its official Android sample uses
   **0483:5740**, named `VENDOR_ELZAB` / `ELZAB_VIRTUAL_COM`
   (`USB/elzab-usbk10/ANDROID/src/main/java/com/hoho/android/usbserial/driver/UsbId.java:52-53`),
   registered specifically with CDC ACM (`.../CdcAcmSerialDriver.java:352-355`);
   the sample offers a Zeta/D10 test activity
   (`.../pl/elzab/virtualcomsample/activity/DeviceSelectionActivity.java:21-22,40-46`).
   In the real USB registry 0483 is **STMicroelectronics**, 5740 is generic
   **Virtual COM Port**, not an ELZAB-owned VID (`USB/linux-usb.ids:3531,3569`).
   This is evidence of a generic MCU CDC identity, not proof of an external
   USB–RS232 bridge chip.
4. Zeta ONLINE manual specifically directs installing `usb_cdc_ser.inf` for its
   virtual serial port, independently of optional RNDIS networking
   (`USB/elzab-zeta.txt:551-568`,
   [manual](https://www.elzab.com.pl/download/io/rozszerzona_instrukcja_obslugi_elzab_zeta_online.pdf)).
   T10 manual instead documents RNDIS/TCPIP only
   (`USB/elzab-t10.txt:357-358,1368-1370`).

#### Per requested model result

- **Model — Zeta (legacy)**: **Proven USB serial evidence**: Vendor legacy
  package/sample uses STM CDC 0483:5740; **Exact normal VID:PID +
  manufacturer/product USB descriptors**: VID:PID source above; exact
  descriptors `[UNVERIFIED]`.
- **Model — D10 (legacy, not established as ONLINE)**: **Proven USB serial
  evidence**: Same legacy package/sample; **Exact normal VID:PID +
  manufacturer/product USB descriptors**: VID:PID source above; exact
  descriptors `[UNVERIFIED]`.
- **Model — Zeta ONLINE**: **Proven USB serial evidence**: Model manual confirms
  virtual serial, HCC CDC driver; **Exact normal VID:PID + manufacturer/product
  USB descriptors**: `[UNVERIFIED]`; neither generic CDC INF nor bootloader INF
  establishes it. Do not copy legacy 0483:5740 without device evidence.
- **Model — Mera ONLINE**: **Proven USB serial evidence**: Shared vendor ONLINE
  CDC package; **Exact normal VID:PID + manufacturer/product USB descriptors**:
  `[UNVERIFIED]`; no model-specific ID/descriptor mapping obtained.
- **Model — Cube ONLINE**: **Proven USB serial evidence**: Shared vendor ONLINE
  CDC package; **Exact normal VID:PID + manufacturer/product USB descriptors**:
  `[UNVERIFIED]`; no model-specific ID/descriptor mapping obtained.
- **Model — T10 ONLINE**: **Proven USB serial evidence**: Shared download
  heading, but model manual only microUSB RNDIS/TCPIP; **Exact normal VID:PID +
  manufacturer/product USB descriptors**: CDC serial itself `[UNVERIFIED]`;
  don't invent a serial identity for a documented network interface.

No unique manufacturer/product string for generic ELZAB normal CDC operation was
found in these vendor files/manuals or targeted web searches (`ELZAB VID/PID`,
`ELZAB CDC-Echo/idVendor`). The installation guide says to find **CDC-Echo
device** (`USB/elzab-install.txt:126-127`), but is a generic installation guide,
not a per-model descriptor specification. This cannot safely be an ELZAB
allowlist string.

### A5. RS232, adapters and model-specific physical connections

- **Brand/model — Posnet Temo ONLINE**: **Documented serial/USB connection**:
  Mini USB; optional WiFi/Bluetooth; no RS232 listed; **Evidence**:
  `USB/posnet-temo-page.txt:139-143`,
  [vendor](https://www.posnet.com.pl/en/produkty/product/361/temo-online).
  Absence of RS232 in this spec is not proof about all revisions `[UNVERIFIED]`.
- **Brand/model — Posnet Thermal HD ONLINE**: **Documented serial/USB
  connection**: Ethernet, **2xRS232**, USB; **Evidence**:
  `USB/posnet-thermal-page.txt:139-143`,
  [vendor](https://www.posnet.com.pl/en/produkty/product/371/thermal-hd-online).
  Exact printer-side connector shape from this page `[UNVERIFIED]`.
- **Brand/model — Posnet Ergo ONLINE**: **Documented serial/USB connection**:
  Cash register; USB, optional modules with 2xRS232; **Evidence**:
  `USB/posnet-ergo-page.txt:2,75,89-91,188-193`. Printer-protocol suitability
  `[UNVERIFIED]`.
- **Brand/model — Novitus HD II ONLINE**: **Documented serial/USB connection**:
  USB default, can switch to RS232/LAN/WiFi/BT; **2xRS232**; **Evidence**:
  `USB/novitus-hd2-page.txt:57-59,348-349`,
  [vendor](https://novitus.pl/hd-ii-online-zintegrowany).
- **Brand/model — Novitus DEON ONLINE**: **Documented serial/USB connection**:
  V.24/RS232C + USB-B; cable supplied; PC software may use RS232/USB;
  **Evidence**: `USB/novitus-deon.txt:1061-1063,1685-1686,1743-1768`,
  [manual](https://novitus.pl/file/download/6c00d097-4e0c-11ed-b3b1-005056904019).
- **Brand/model — Novitus BONO ONLINE**: **Documented serial/USB connection**:
  **2xRS232**, USB Host A + Device B, LAN; **Evidence**:
  `USB/novitus-bono-page.txt:211,294-295`,
  [vendor](https://novitus.pl/bono-online).
- **Brand/model — Novitus POINT**: **Documented serial/USB connection**: RS232
  RJ45 COM1 and USB-B (two serial ports); supplied RS232 cable; pin table **RJ45
  -> PC DB9**; **Evidence**:
  `USB/novitus-point.txt:765-783,1296-1297,1375-1400,4681-4697`,
  [manual](https://point.novitus.pl/files/Instrukcja_Obslugi_POINT_v03_20231114.pdf).
- **Brand/model — ELZAB Zeta ONLINE**: **Documented serial/USB connection**:
  USB-B virtual serial; RS232 **RJ45 -> PC DSub9** using PC RS/UTP adapter;
  **Evidence**: `USB/elzab-zeta.txt:551-581,1958-1960`.
- **Brand/model — ELZAB Mera ONLINE**: **Documented serial/USB connection**:
  USB + RS232 **RJ45 -> PC DSub9**, adapter/cable bought separately;
  **Evidence**:
  `USB/elzab-mera.txt:556-563,1855`,
  [manual](https://www.elzab.com.pl/download/io/rozszerzona_instrukcja_obslugi_elzab_mera_online.pdf).
- **Brand/model — ELZAB Cube ONLINE**: **Documented serial/USB connection**:
  USB + RS232 **RJ45 -> PC DSub9**, adapter/cable bought separately;
  **Evidence**:
  `USB/elzab-cube.txt:568-586,1820`,
  [manual](https://www.elzab.com.pl/download/io/rozszerzona_instrukcja_obslugi_elzab_cube_online.pdf).
- **Brand/model — ELZAB T10 ONLINE**: **Documented serial/USB connection**:
  microUSB RNDIS/TCPIP to D2 mini POS; **Evidence**:
  `USB/elzab-t10.txt:357-365,1368-1370`. No RS232 documented.
- **Brand/model — ELZAB D10 legacy**: **Documented serial/USB connection**:
  Vendor has a dedicated connections package; **Evidence**:
  `USB/elzab-programs-page.txt:263-264`,
  [package](https://www.elzab.com.pl/ftp/kody/connections_D10.zip). RS232
  connector shape not established in this investigation `[UNVERIFIED]`.

RS232 is not theoretical: POINT and DEON manuals list supplied RS232 cables, and
ELZAB sells the universal FTDI232RL converter explicitly for printers
(`USB/novitus-point.txt:1296-1297`, `novitus-deon.txt:1685-1686`,
`elzab-adapter-page.txt:73-78`). A secondary
[Windows driver
listing](https://www.driveridentifier.com/scan/elzab-konwerter-usb-rs232-v2-driver/download/1228403692/EC3CBAA4EBE84CE3A94B42A64FB91612/USB/VID_0403&PID_6001)
associates **ELZAB KONWERTER USB-RS232 v.2** with **0403:6001**; this is adapter
identity, not printer identity. Generic third-party adapters can have different
IDs/strings `[INFERENCE]`, so there is no universal printer-specific string for
adapters.

### A6. Workspace serialport version, platform metadata and current filtering

#### Version and available fields

- Workshop `Cargo.toml:134-135`:
  `serialport = { version = "4.10.1", default-features = false }`, comment says
  sysfs avoids libudev; `Cargo.lock:5338-5341` resolves 4.10.1; posnet
  Cargo.toml:17 uses the workspace dependency.
- Source root (abbreviated `SP`) =
  `/home/g4m3r/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/serialport-4.10.1`.
  Public `UsbPortInfo` contains `vid: u16`, `pid: u16`, optional
  `serial_number`, `manufacturer`, `product` (`SP/src/lib.rs:819-829`). None of
  these five requires a feature. Interface/location are separate feature-gated
  extras (`:830-838`). Public types are documented
  [upstream](https://github.com/serialport/serialport-rs/blob/main/_autodocs/api-reference/port-info.md);
  exact installed-version implementation below is stronger than general docs.
- **Linux, current no-libudev build:** directly scans `/sys/class/tty`;
  distinguishes subsystem types (`pci` -> PciPort; `pnp`, `serial-base`, `amba`
  -> Unknown; `usb`/`usb-serial` -> USB), reads VID/PID and optional
  serial/product/manufacturer from sysfs
  (`SP/src/posix/enumerate.rs:643-660,692-739,742-782`). Thus disabling libudev
  **does not remove those five USB fields** in 4.10.1. Missing/invalid VID/PID
  prevents an entry being classified as USB (`:716-720`); strings can be absent
  (`:725-727`).
- **macOS:** IOKit parent USB interface/device provides `idVendor`, `idProduct`,
  `USB Serial Number`, `USB Vendor Name`, `USB Product Name`; string reads may
  fail, VID/PID default to zero on missing property
  (`SP/src/posix/enumerate.rs:446-477`). Bluetooth is classified by
  IOBluetoothSerialClient parent; remaining devices PciPort (`:478-481`).
- **Windows:** USB instance/parent hardware IDs produce VID/PID/serial
  (`SP/src/windows/enumerate.rs:157-186`); `manufacturer` is **SPDRP_MFG**,
  `product` is **SPDRP_FRIENDLYNAME** (`:410-430`), i.e. driver/OS properties,
  not guaranteed identical to actual USB iManufacturer/iProduct strings. Parsing
  failure returns Unknown (`:430`). `[INFERENCE]` Exact cross-OS product-string
  equality is an unreliable filtering contract.
- `SerialPortType` variants are UsbPort, **PciPort** (“permanent port”),
  **BluetoothPort**, **Unknown** (“can't be determined how ... connected”), not
  “printer/non-printer” (`SP/src/lib.rs:1005-1026`). A Linux `/dev/ttyS0` is the
  traditional first UART/Windows COM1
  ([Red
  Hat](https://docs.redhat.com/en/documentation/red_hat_enterprise_linux/5/html/virtualization/sect-virtualization-troubleshooting_xen-troubleshooting_with_serial_consoles);
  [Arch](https://wiki.archlinux.org/title/Working_with_the_serial_console));
  `/dev/ttyS*` may be PCI/PNP/serial-base under this implementation, not
  necessarily all PciPort. A name COM1 alone cannot prove transport because
  Windows uses instance IDs/properties, not its numeric name.
  `/dev/cu.Bluetooth-Incoming-Port` is a macOS Bluetooth serial endpoint, not
  USB printer evidence
  ([Apple community explanation](https://discussions.apple.com/thread/7821085));
  source classifies Bluetooth ancestry as BluetoothPort
  (`SP/src/posix/enumerate.rs:478-479`).

#### Existing workshop enumeration and callers

- `crates/posnet/src/session.rs:67-78`: call
  `serialport::available_ports().unwrap_or_default()`, **retain all port
  kinds**, drop `/dev/tty.*` only on macOS, sort USB first, then throw away
  every property except `port_name`. Enumeration errors become an empty list. No
  VID/PID, vendor, product or Bluetooth/PCI/Unknown filter exists.
- Discovery: `SerialLink::candidates()` simply returns `serial_ports()`
  (`session.rs:62-64`), so it inherits exactly that same lack of brand
  filtering.
- CLI: `crates/desktop/src/cli.rs:282-288` calls `session::serial_ports()`,
  outputs names in JSON/human format, adds no filtering and does not open ports.
- GUI: `crates/desktop/src/application.rs:234` calls the same `serial_ports()`
  and sets the picker. No second port filter exists at this boundary.
- `[INFERENCE]` A new candidate-classification function should therefore be
  shared by all three callers, preserve metadata until after classification, and
  treat any eventual read-only protocol identification separately from GUI/CLI
  enumeration (printer I/O remains Worker-only: workshop `.omp/RULES.md:13-18`).
  It must not silently introduce probing on the GUI thread or in the config-only
  ports CLI.
- `[INFERENCE]` Requirement decision needed for Main: (a) **direct-USB-only**
  discovery/picker (explicitly excludes valid RS232/adapter paths and cannot yet
  have a complete ELZAB model allowlist), or (b) conservatively retain ports
  that **could** host a supported serial printer, using safe Worker-only
  protocol identification to prove identity before operations. Exact “only
  printers, zero false positives, all serial paths” cannot be fulfilled by the
  available OS metadata. An unknown port is lack of evidence, not evidence of
  impossibility.

## B. Backend required telemetry vendor enum footprint

### B1. Actual current locations and behavior

The requested `crates/fiscal/src/printers/...` location has moved: current code
is **`crates/fiscal/src/receipts/...`** (confirmed by scoped search and concrete
paths below).

- **Telemetry type:** backend `crates/fiscal/src/receipts/model.rs:96-109`;
  required fields limits/characters/serial/model/firmware/appVersion/vatSlots,
  no vendor. Both serde and deserr use camelCase + deny_unknown_fields
  (`:96-98`). `Printer` response is `:32-55`:
  model/firmware/appVersion/deviceSerial are nullable `Option<Code>` but marked
  required properties. `PrinterStatus` wraps it with online/lastSeenAt/updatedAt
  (`:56-64`).
- **Endpoints:** `receipts/http.rs:362-392`: `/agent/session` extracts
  `Json<Telemetry>` and calls `printers::handshake`; heartbeat extracts the same
  type and delegates to session. Agent router registers both (`:497-500`). Staff
  routes are `/staff/fiscal/printers`, not `/manager` (`:480-484`); fiscal lib
  exposes Staff and Agent audiences (`crates/fiscal/src/lib.rs:40-52`).
- **Validation/storage:** service `receipts/printers.rs:139-154` validates form,
  locks/read printer, rejects serial mismatch, writes handshake, reloads
  response. Repository `receipts/repo/printers.rs:170-200` writes
  device_serial/model/firmware/app_version, limits/chars and last_seen;
  `:201-230` replaces VAT slots. Query
  `crates/fiscal/queries/receipts/printers.sql:1-24` selects
  model/firmware/etc.; `receipts/repo.rs:43-80` maps `PrinterRow` into the
  shared Printer DTO.
- **Initial schema:** `migrations/0010_fiscal.sql:21-44`: nullable
  `device_serial`, `model`, `firmware`, `app_version`, limits/chars; no vendor
  column/type. Live device serial is globally unique (`:49-50`) and permanently
  pinned by update guard (`:357-362`). `0012_fiscal_company_name_limit.sql:5-25`
  adds the fourth capability and invalidates existing partial telemetry rather
  than guessing; `0013_fiscal_printer_pairing.sql:5-15` adds pairing fields.
  Current audit trigger exclusions are `0013...:32-33` (model, firmware etc.
  excluded; new vendor decision must be included/excluded intentionally).
- **API documents:** backend `openapi/agent.json:1638-1688` Telemetry and
  `:1347-1465` Printer; backend `openapi/staff.json:38462-38580` Printer
  includes model/firmware. The Rust backend has no manager audience document;
  web-facing staff API uses staff.json. The separate current frontend still uses
  the old manager schema (see B5).

### B2. The rejection test and why it exists

- Exact test: `crates/fiscal/src/receipts/printers/tests.rs:42-43`,
  **`session_and_heartbeat_replace_typed_slots_without_legacy_fields`**. It adds
  `"vendor":"posnet"`, POSTs session, expects HTTP 400, `validation_failed` with
  pointer `/vendor` (`:140-152`). This is not an enum-value rejection: **vendor
  is entirely an unknown property**, because Telemetry denies unknown fields and
  has no vendor (`model.rs:96-109`).
- Another test explicitly asserts response has no vendor alongside removed
  deviceModel/agentVersion/vatRates (`receipts/tests/access.rs:259-266`). Exact
  Printer JSON before/after handshake is pinned
  (`receipts/printers/tests.rs:47-62,85-100`) and new-pair shape is pinned
  (`receipts/tests/pairing.rs:163-174`).
- `git blame -L130,152` attributes addition of the vendor rejection to
  **de78b4360ef73bdf68ec329ff03e00bde5aecfac (2026-10-20), “Story/improvments
  (#5)”**. `git log --all -S'vendor' -- crates/fiscal ...` locates the change.
  `git show de78b436 -- crates/fiscal/src/receipts/model.rs` shows vendor
  removed from **both** Printer (`Option<FiscalVendor>`) and Telemetry
  (`String`), along with `device_model -> model`,
  `agent_version -> app_version`, old vat_rates -> typed vat_slots and removed
  vendor filtering. The same commit introduces the named rejection test. Its
  parent `crates/db/src/enums.rs` contains `FiscalVendor` with
  Posnet/Novitus/Elzab; the current file does not (`current enums.rs:109-112`
  lists only other fiscal enums).
- Commit message includes “**feat: rewrite fiscal printing with typed receipt
  lines, VAT slots and buyer NIP**” and “**refactor(fiscal): audit — drop unused
  columns/indexes, one live pairing per serial**” (full commit message returned
  by git, transcript artifact `artifact://11`). **There is no specific written
  rationale saying “vendor removed because only Posnet is supported” in that
  message.** `[INFERENCE]` The evidence supports deliberate legacy-contract
  cleanup during fiscal rewrite; a stronger product rationale is `[UNVERIFIED]`.
  Do not present an assumed rationale as history.

### B3. Files a persisted required telemetry vendor needs to touch

`[INFERENCE]` Proposed shape: Postgres/Rust `FiscalVendor` enum
`posnet|novitus|elzab`, **required Telemetry.vendor**, nullable-but-present
`Printer.vendor` (new/unseen/legacy printer rows cannot honestly have a required
non-null vendor). This footprint is for persistence + exposure, not merely
accepting and discarding a field.

#### Backend source/schema/generated files

(complete identified direct footprint)

- **File — new forward-only `migrations/<next>_fiscal_printer_vendor.sql`**:
  **Required change / source evidence**: Create enum + nullable column; explicit
  audit policy; populated upgrade behavior. Do not rewrite 0010/0012/0013:
  forward-only rule `AGENTS.md:151-155`; expand/deploy/contract
  `.omp/rules/database.md:116-120`.
- **File — `crates/db/src/enums.rs`**: **Required change / source evidence**:
  Add typed `FiscalVendor` using existing db_enum macro (SQL/serde/deserr/schema
  support `:4-50`; current fiscal enums `:109-112`).
- **File — `crates/db/src/enums/tests.rs`**: **Required change / source
  evidence**: Register new enum in all-variant scalar/array/text/JSON/deserr
  parity test (`:38-65,94-97`).
- **File — `sqlx.types.toml`**: **Required change / source evidence**: Add
  `fiscal_vendor -> db::enums::FiscalVendor` mapping, matching other fiscal
  types (`:188-191`).
- **File — generated `sqlx.toml`**: **Required change / source evidence**:
  Regenerate scalar/array/per-column overrides; examples `:1068-1077`;
  instructions `AGENTS.md:166-171`, `.omp/rules/database.md:64-69`.
- **File — `crates/fiscal/src/receipts/model.rs`**: **Required change / source
  evidence**: Add required typed vendor to Telemetry, nullable required property
  to Printer (`:32-55,96-109`).
- **File — `crates/fiscal/src/receipts/repo/printers.rs`**: **Required change /
  source evidence**: Bind/store vendor in handshake UPDATE (`:170-200`).
- **File — `crates/fiscal/queries/receipts/printers.sql`**: **Required change /
  source evidence**: Select vendor with model/firmware (`:1-12`).
- **File — `crates/fiscal/src/receipts/repo.rs`**: **Required change / source
  evidence**: Add nullable vendor to PrinterRow and DTO mapping (`:43-75`).
- **File — `crates/fiscal/src/testing.rs`**: **Required change / source
  evidence**: Populate required vendor in actual `Telemetry { ... }` factory
  (`:113-127`).
- **File — `crates/fiscal/src/receipts/tests.rs`**: **Required change / source
  evidence**: Add vendor to shared JSON telemetry helper (`:179-190`).
- **File — `crates/fiscal/src/receipts/printers/tests.rs`**: **Required change /
  source evidence**: Replace vendor-is-unknown rejection with valid enum
  roundtrips/missing/bad/unknown-field cases; update both exact response shapes
  (`:42-100,140-152`). Test session AND heartbeat persist correct brand, no
  invalid update, and legacy null -> observed vendor.
- **File — `crates/fiscal/src/receipts/tests/access.rs`**: **Required change /
  source evidence**: Remove vendor from removed-fields list and assert returned
  value (`:259-266`).
- **File — `crates/fiscal/src/receipts/tests/pairing.rs`**: **Required change /
  source evidence**: Add `vendor:null` to exact unpaired-printer response
  (`:163-174`).
- **File — `crates/fiscal/src/receipts/tests/storage.rs`**: **Required change /
  source evidence**: Add populated migration test following existing fiscal
  upgrades (`:203-242,269-291`); prove old rows, unpaired rows, deleted rows and
  existing queue/journal invariants unaffected.
- **File — `crates/legacy-import/src/archive.rs`**: **Required change / source
  evidence**: Preserve/validate vendor in shared printer_values; current
  validation allows only supported strings/null (`:1417-1423`) but
  printer_values never writes it (`:1431-1456`). Direct loader and verifier both
  use this mapping (`:1742-1746,2289-2297`).
- **File — `crates/legacy-import/src/project_archive.rs`**: **Required change /
  source evidence**: Migrate projection's `_legacy_vendor` metadata (`:604-609`)
  to real vendor from shared printer_values; remove obsolete metadata key once
  direct column is retained.
- **File — `crates/legacy-import/src/archive/tests.rs`**: **Required change /
  source evidence**: Extend printer_values normalization test (`:631-638`) for
  each valid vendor, null, invalid value and persistence mapping.
- **File —
  `.sqlx/query-3498434d1d1f15befb9ccdb784da53f50fcadef5110f874413bbe1ee8822f01b.json`**:
  **Required change / source evidence**: Existing handshake query metadata
  (`:3`) becomes obsolete; regenerate, do not hand-edit.
- **File —
  `.sqlx/query-368c91a1030f7fa9ac683710e4760afc680e20f2c55685fc7de08f3c918ff436.json`**:
  **Required change / source evidence**: Existing printer SELECT metadata (`:3`,
  model/firmware columns `:52-69`) becomes obsolete; regenerate. Other generated
  cache changes depend on final SQL/type/schema generator output.
- **File — `openapi/agent.json`, `openapi/staff.json`**: **Required change /
  source evidence**: Regenerate shared Printer + required Telemetry +
  FiscalVendor schema. Current definitions cited in B1. No reason to hand-edit
  generators/handler code just to add a derived DTO field.

Conditional domain-file change: `crates/fiscal/src/receipts/printers.rs:139-154`
only needs editing if policy is to **pin vendor once observed** (strongly
recommended `[INFERENCE]` with device identity); otherwise unchanged because it
passes complete Telemetry. Existing permanent identity is serial-only, globally
unique (`migrations/0010_fiscal.sql:49-50,360-362`). Whether serial is globally
unique **across brands** is `[UNVERIFIED]`; adding composite `(vendor,serial)`
identity would be a separate public/data contract decision, not an automatic
telemetry-field change.

Intentionally unchanged: existing immutable migrations 0010/0012/0013,
`receipts/http.rs` (both endpoints already share DTO),
`crates/fiscal/src/lib.rs` (schema reachability derived through DTO),
`crates/holuto/src/lib.rs` and `cli.rs` (existing exporter handles new schema),
unrelated receipt snapshots (vendor is printer telemetry, not a fiscal record
field), translations (unless a new domain error is introduced), and legacy
historical behavior documentation (it already records the actual old vendor
field). The new migration can decide audit behavior without rewriting old
trigger migrations.

#### Workshop contract consumers affected in the subsequent implementation

- Vendored `openapi/agent.json`; generated wire types arise via
  `crates/agent/build.rs` (README:218-224). Add required vendor in
  `crates/agent/src/api/model.rs:79-89` Telemetry::new and populate from probed
  device in `crates/agent/src/agent.rs:115-118` (current model/firmware
  assignments; coupling map §2h). Constructor must not silently default every
  new vendor to Posnet `[INFERENCE]`.
- Fake backend `crates/agent/src/testing.rs:639-649` copies telemetry names into
  Printer and `:791-795` constructs default Printer JSON; update both.
- Contract fixtures/helpers `crates/agent/src/api/tests.rs:19-23`,
  `api/contract/tests.rs:19-23`, `testing/tests.rs:29-32`; add enum coverage and
  response parsing as needed. Workshop `README.md:230-246` sample must show
  vendor. `crates/agent/build.rs` may need an explicit mapping only if the enum
  should reuse the new driver-domain enum; it should otherwise generate it
  normally (type transformations documented in
  `posnet-coupling.md:260-261`).

### B4. Legacy import `_legacy_vendor`: what currently survives

- Source legacy schema allows vendor null or posnet/novitus/elzab; historical
  docs backend `crates/legacy-import/docs/fiscal.md:69-83` record this and old
  device_model/firmware fields. This docs file describes **historical**
  behavior, so adding current vendor storage does not make it wrong.
- `archive.rs:1417-1423` validates source vendor but
  `printer_values(:1431-1456)` only emits name, token_hash, last_seen_at,
  device_serial/model/firmware; it discards vendor for target DB. The direct
  archive loader calls printer_values and inserts target at `:1742-1746`.
- Projection path adds `_legacy_vendor = row["vendor"]`
  (`project_archive.rs:604-609`). Bulk writer deliberately removes
  underscore-prefixed keys unless the target table actually has such a column
  (`bulk_write.rs:353-364`). So `_legacy_vendor` is **projection metadata, not
  stored telemetry**. It is not a hidden populated DB vendor column that can
  simply be renamed.
- `[INFERENCE]` Once real `vendor` exists, put canonical validated source vendor
  in shared printer_values so both projection and direct loader/verifier agree,
  and delete obsolete `_legacy_vendor`. Preserve null when source vendor is
  genuinely unknown. Do not default legacy Novitus/Elzab to Posnet or infer
  brand from arbitrary model text without evidence.

### B5. OpenAPI generation and web frontend mismatch

- Backend `AGENTS.md:184-186` requires **`cargo run -p holuto -- openapi`**
  after HTTP contract changes. CLI `crates/holuto/src/cli.rs:24-27,61` has
  Openapi subcommand, default output `openapi`;
  `crates/holuto/src/lib.rs:195-205` builds audience documents, iterates all
  audiences, pretty-prints `<audience>.json`. It starts neither HTTP nor the
  worker (`cli.rs:24`). No command was executed in this investigation.
- Current Rust backend's staff Printer response exposes model/firmware via
  common DTO/query (`model.rs:49-53`, `queries/receipts/printers.sql:6-8`,
  `openapi/staff.json:38477-38568`). There is **no manager document/route
  equivalent** to the existing frontend's old one; fiscal audience split is
  Staff/Agent (`fiscal/src/lib.rs:40-52`).
- A real sibling **frontend** exists. `frontend/AGENTS.md:3,17-18` says it
  currently targets `../api` (Django), generates request/response types from
  checked-in OpenAPI, requires `bun run api:sync`, and deploys API first.
  `scripts/api-types.ts:8` role list is **manager/mechanic/customer**, not new
  backend staff/customer/ops/agent; `:58-66` copies exactly those filenames from
  API_DIR (default ../api). Simply pointing API_DIR to `../backend` is
  insufficient because manager.json/mechanic.json do not exist there
  `[INFERENCE]`.
- `frontend/openapi/manager.json:44341-44425` already has old FiscalPrinterList
  `device_model`, `firmware`, nullable vendor enum **elzab/novitus/posnet**;
  vendor query filter at `:27427-27430`. Generated (gitignored)
  `frontend/src/api/manager/gen/types.gen.ts:750-779` confirms this. It is
  **not** proof the Rust backend currently supports vendor.
- Printer UI is
  `frontend/src/routes/_authenticated/manager/settings/fiscal-printers.tsx:105-110`:
  displays **device_model**, not new `model`; it fetches old
  `integrationsFiscalPrintersListOptions` (`:8-12,43`). Its tests
  (`fiscal-printers.test.tsx:184-190`) and sale modal test
  (`service-orders/-components/ModalFiscalReceipt.test.tsx:122-131`) use old
  snake_case fields; the latter already supplies vendor posnet. Generated
  manager clients export old integrations paths
  (`src/api/manager/gen/types.gen.ts:6833-6835,6897-6899`).
- `[INFERENCE]` A backend+Workshop vendor-only change does **not** require
  frontend source migration: that app currently talks to a different API and
  already knows the vendor enum. If migrating this frontend onto Rust backend is
  intended, it is a **separate full API cutover**, not adding a vendor field.
  Affected fiscal subset includes `frontend/openapi/manager.json` (or new staff
  schema after role migration), `scripts/api-types.ts`,
  `src/api/manager/{index.ts,config.ts}` and generated `gen/`,
  `src/api/manager/tags/integrations.ts`, printer settings route + its test,
  sale `ModalFiscalReceipt.tsx` + test, and API codegen role configuration.
  Existing routeTree/menu/access changes depend on the wider role migration and
  must not be smuggled into telemetry scope. No vendor-label UI is currently
  requested.

### B6. Migration and rollout recommendations grounded in current rules

1. **Nullable stored vendor is necessary:** staff creates a printer from name
   alone (`model.rs:65-70`; repository create `repo/printers.rs:23-25`), and
   tests explicitly expose null device metadata before session
   (`printers/tests.rs:47-62`). Required request field does **not** imply all
   stored rows can be NOT NULL. Add `Option<FiscalVendor>` to response and keep
   property present/null (backend HTTP convention `AGENTS.md:183`).
2. **Do not default old rows to posnet.** There is no current vendor column
   (`0010_fiscal.sql:21-44`), old importer validates all three brands
   (`archive.rs:1417-1423`), and model is arbitrary Code (`model.rs:105`;
   response :49). `[INFERENCE]` Existing rows stay null until fresh verified
   telemetry, or get only evidence-backed explicit backfill; imported source
   vendor can be preserved for future imports.
3. **Deploy sequencing is not solved just by “backend first.”** Old Workshop has
   no vendor in Telemetry (`workshop/openapi/agent.json:1638-1688`;
   README:230-246), while current backend rejects any added vendor.
   `[INFERENCE]` A required-field clean cutover needs coordinated backend/agent
   rollout (old agents intentionally fail the new handshake), or an explicitly
   approved temporary accept-optional rollout followed by requirement
   tightening. Do not silently make vendor optional in the final contract.
   Preserve operation journals; blocking handshake is not permission to replay a
   device command.
4. **Forward-only migration must run before the new binary and work with the
   previous binary** (`backend/.omp/rules/database.md:116-120`): nullable
   column/type satisfies old UPDATEs. If readiness must require known vendor,
   decide and test how existing queued/taken/unknown rows behave before adding a
   database guard; no printer operation may be retried or dropped to manufacture
   telemetry (workshop README:168-205).
5. **Consider pinning observed vendor with serial, but ask before changing
   identity uniqueness/contracts.** Existing DB already pins serial
   (`0010:360-362`); actual cross-brand unique serial guarantee is
   `[UNVERIFIED]`. At minimum telemetry must not claim a different vendor while
   an unresolved physical-printer operation is active `[INFERENCE]`.
6. **Verification Main should run after implementation, not performed here:**
   new enum DB/serde/deserr parity; required/invalid enum rejection on
   session+heartbeat without mutation; all three vendor roundtrips through
   agent/staff responses; unpaired vendor null; populated migration; both
   importer paths preserving vendor/null; refreshed sqlx mappings/cache and
   regenerated OpenAPI; Workshop generated-wire/fake contract tests;
   simulator-only driver/discovery tests. No real-printer commands.
