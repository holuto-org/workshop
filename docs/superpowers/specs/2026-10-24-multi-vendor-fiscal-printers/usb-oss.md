# USB identification and native-UART filtering: independent-source findings

Research date: 2026-10-24. Read-only: no printer/device was opened, no printer
command was issued, no repository file was changed, no tests/builds were run.
This is behavioral research, not copied implementation code.

## Result

- **SETTLED:** Workshop uses `serialport` **4.10.1**, without libudev. Native
  Windows COM ports are **Unknown, not PciPort**. Linux native UARTs can be
  **PciPort or Unknown**; macOS labels every non-USB/non-Bluetooth serial
  service **PciPort**. Therefore `PciPort` is not a portable native-RS232
  filter.
- **SETTLED:** Linux can passively distinguish kernel-recognized UARTs from
  `PORT_UNKNOWN` slots using `/sys/class/tty/<name>/type`: decimal **0 =
  PORT_UNKNOWN**. This avoids opening ports merely to populate a GUI. It proves
  kernel recognition, not an attached printer, exposed connector, RS232
  electrical levels, or free/usable port.
- **SETTLED:** Novitus's official drivers supplied for all four requested models
  match **1a28:2512**, communication interfaces **00 / 02** (A / B). These are
  driver-supported identities; actual per-model USB manufacturer/product strings
  remain unobserved.
- **New observed evidence:** a real Posnet Thermal XL2 forum trace establishes
  **1424:10b0**, manufacturer **`Posnet`**, product **`Posnet D`**,
  communication interface **00**, interface string **`Posnet D Serial Port`**,
  CDC ACM, bcdDevice **0200**. It does **not** establish Temo ONLINE's exact
  descriptors.
- **STILL OPEN:** exact observed VID:PID + USB descriptor strings for **Temo
  ONLINE**, and normal-mode IDs/strings/interfaces for **Elzab Zeta/Mera/Cube
  ONLINE**. No reliable model-specific traces were found. Do not silently
  promote legacy IDs or INF display names into these gaps.

## Sources

`USB/` is the extracted USB evidence namespace described below. `SP/` names
an unpacked `serialport-4.10.1/` source directory.
Line numbers below refer to these local source snapshots. Version-pinned
upstream source citations use commit
**41496a9958aa47ff930a0ac5e659110e35d3d752**, recorded in
`SP/.cargo_vcs_info.json:1-5`.

### Primary sources used

1. [Posnet CDC driver
   ZIP](https://www.posnet.com.pl/files/download/pos-h-dev-23-015-sterownik-usb-posnetcdc.zip):
   `USB/posnet/posnetCDC15v.utf8.inf`, DriverVer 06/22/2018,2.23.15; decoded
   copy of `posnetCDC15v.inf`.
2. [Novitus online x64
   ZIP](https://novitus.pl/file/download/3180ebbe-ead8-11eb-a3d6-005056904019):
   `USB/novitus/drukarki_online_x64/novitus.utf8.inf`; DriverVer
   04/28/2014,1.3.6.
   [POINT x64 ZIP](https://point.novitus.pl/files/drukarki_online_x64.zip):
   matching `USB/point/drukarki_online_x64/novitus.utf8.inf`.
3. [ELZAB online CDC
   ZIP](https://www.elzab.com.pl/download/program/Usb_cdc_ser.zip):
   `USB/elzab/usb_cdc_ser.inf`; nested `atm6124_cdc_signed/atm6124_cdc.inf`.
4. [ELZAB legacy USB Windows/Android
   ZIP](https://www.elzab.com.pl/ftp/uzyt/usbk10.zip):
   `USB/elzab-usbk10/ANDROID/src/main/...`.
5. [`serialport` POSIX source, pinned
   commit](https://github.com/serialport/serialport-rs/blob/41496a9958aa47ff930a0ac5e659110e35d3d752/src/posix/enumerate.rs),
   [Windows
   source](https://github.com/serialport/serialport-rs/blob/41496a9958aa47ff930a0ac5e659110e35d3d752/src/windows/enumerate.rs),
   [public
   types](https://github.com/serialport/serialport-rs/blob/41496a9958aa47ff930a0ac5e659110e35d3d752/src/lib.rs).
   Source is present in the Cargo registry; no download/build was necessary.
6. [Linux sysfs tty
   ABI](https://www.kernel.org/doc/Documentation/ABI/testing/sysfs-tty),
   [Linux v6.12
   serial_core.c](https://github.com/torvalds/linux/blob/v6.12/drivers/tty/serial/serial_core.c),
   [Linux v6.12
   serial.h](https://github.com/torvalds/linux/blob/v6.12/include/uapi/linux/serial.h).

#### Reproducing local snapshots

Download source #1 as `USB/posnet.zip`, #2 as `USB/novitus.zip` and
`USB/point.zip`, #3 as `USB/elzab.zip`, and #4 as
`USB/elzab-usbk10.zip`. Use `unzip <zip> -d <directory>`, with directory
names `USB/posnet/`, `USB/novitus/`, `USB/point/`, `USB/elzab/`, and
`USB/elzab-usbk10/`, respectively. Original INF members are
`posnetCDC15v.inf`, `drukarki_online_x64/novitus.inf`,
`drukarki_online_x64/novitus.inf`, and `usb_cdc_ser.inf` in the first four
packages. The last package preserves its `ANDROID/` and `WINDOWS/` member
paths. Extract `USB/elzab/atm6124_cdc_signed.zip` with `USB/elzab/` as the
destination to obtain `atm6124_cdc_signed/atm6124_cdc.inf`.

The `.utf8.inf` aliases are CP1250-to-UTF-8 copies of the corresponding
original `.inf` files: use
`iconv -f CP1250 -t UTF-8 <original.inf> > <copy.utf8.inf>`, preserving
blank lines. These are static files, not installed/executed.

For SP, the original research read Cargo's registry copy without downloading
or building. A fresh reader can instead download
[the exact serialport 4.10.1 crate][serialport-crate] as
`serialport-4.10.1.crate`, then use
`tar -xzf serialport-4.10.1.crate`; its top directory is
`serialport-4.10.1/`. The pinned GitHub URLs in source #5 provide the same
upstream source provenance. The Linux `serial_core.c` and `serial.h` links in
source #6 are pinned to v6.12; read those raw source files directly. Apart
from the aliases below, linked forum/blog/vendor pages are unversioned web
evidence, not local text extracts; headings and post IDs identify passages.

[serialport-crate]: https://static.crates.io/crates/serialport/serialport-4.10.1.crate

- **USB/linux-usb.ids**: [Linux USB registry][linux-usb], version
  2026.06.26; save directly as `linux-usb.ids` without conversion. The
  [maintainer's HTTPS mirror][usb-mirror] is the alternative linked below.
- **USB/elzab-programs-page.txt**: [official ELZAB programs page][programs],
  unversioned HTML snapshot collected 2026-10-24. Fetch UTF-8 HTML with
  User-Agent `Mozilla/5.0`; parse with Python's `html.parser.HTMLParser`.
  Suppress data inside `script` and `style`; for every other nonempty data
  node append `' '.join(data.split())`, preserving node order. Write
  `'Source: ' + url + '\n' + '\n'.join(parts) + '\n'` as UTF-8.
  Mutable vendor pages and registry updates can change current line numbers.

[linux-usb]: http://www.linux-usb.org/usb.ids
[usb-mirror]: https://usb-ids.gowdy.us/usb.ids
[programs]: https://www.elzab.com.pl/pl/strefa-dla-klienta/do-pobrania/programy

License notes: `serialport` MPL-2.0 (`SP/LICENSE.txt:1-2`); ELZAB's bundled
`com/hoho/android/usbserial/driver/UsbId.java:1-19` explicitly
LGPL-2.1-or-later, with LGPL 2.1 text in adjacent `LICENSE.txt`. Linux
`serial_core.c` carries GPL-2.0+; `include/uapi/linux/serial.h` carries GPL-1.0+
WITH Linux-syscall-note. Vendor INFs have not been established as open-source
licensed; use only documented identifiers/behavior, not their implementation.
Forum/blog text is not treated as freely licensed code. No code is proposed for
copying.

## 1. Requested device matrix: evidence versus remaining gaps

- **Requested model — Posnet Temo ONLINE**: **VID:PID evidence**: Shared
  official ONLINE CDC package contains `1424:10b0` generic printer and
  `1424:1015` explicitly Temo **HS**, not a Temo ONLINE mapping [P1];
  **Manufacturer/product evidence**: INF says `POSNET S.A.` / `Posnet D Serial
  Port` for 10b0, but these are Windows properties, not USB descriptors;
  **Interface evidence**: INF 10b0 REV_0300 specifies MI_00; REV_0200 matches
  whole device; **Status of exact observed descriptors**: **STILL OPEN**: no
  observed Temo ONLINE dump found; cannot choose 10b0/1015/1001 as a proven
  exact-model identity
- **Requested model — Novitus HD II ONLINE**: **VID:PID evidence**: Official
  model page supplies 1a28:2512 driver [N1]; **Manufacturer/product evidence**:
  Driver MFG `Novitus`; description A/B below. Actual iManufacturer/iProduct
  **STILL OPEN**; **Interface evidence**: MI_00=A, MI_02=B in official INF;
  **Status of exact observed descriptors**: **SETTLED** driver-supported
  ID/interfaces; **STILL OPEN** actual per-model descriptor strings/revision
  inventory
- **Requested model — Novitus Deon ONLINE**: **VID:PID evidence**: Same official
  ZIP [N1]; **Manufacturer/product evidence**: Same distinction; **Interface
  evidence**: Same INF; integrator config sets printer COM3 (USB1) to PC [N3];
  **Status of exact observed descriptors**: **SETTLED** driver-supported
  ID/interfaces; **STILL OPEN** actual per-model descriptors
- **Requested model — Novitus Bono ONLINE**: **VID:PID evidence**: Same official
  ZIP [N1]; **Manufacturer/product evidence**: Same distinction; **Interface
  evidence**: Same INF; **Status of exact observed descriptors**: **SETTLED**
  driver-supported ID/interfaces; **STILL OPEN** actual per-model descriptors;
  do not substitute Bono **E** 1a28:6010
- **Requested model — Novitus Point**: **VID:PID evidence**: POINT official ZIP
  has same ID/interfaces [N2]; **Manufacturer/product evidence**: Same
  distinction; **Interface evidence**: Same INF; **Status of exact observed
  descriptors**: **SETTLED** driver-supported ID/interfaces; **STILL OPEN**
  actual per-model descriptors
- **Requested model — Elzab Zeta ONLINE**: **VID:PID evidence**: Official normal
  CDC INF is class-only [E1]; legacy Zeta sample's 0483:5740 is not ONLINE
  evidence [E2]; **Manufacturer/product evidence**: No normal-mode observed USB
  strings found; HCC Embedded / CDC Device are INF properties; **Interface
  evidence**: CDC class/subclass/protocol matches, no exact interface number
  established; **Status of exact observed descriptors**: **STILL OPEN**
  normal-mode VID:PID, strings and interface numbers
- **Requested model — Elzab Mera ONLINE**: **VID:PID evidence**: Same class-only
  online package [E1]; **Manufacturer/product evidence**: Same absence of
  observed descriptors; **Interface evidence**: No exact interface number
  established; **Status of exact observed descriptors**: **STILL OPEN**
  normal-mode VID:PID, strings and interface numbers
- **Requested model — Elzab Cube ONLINE**: **VID:PID evidence**: Same class-only
  online package [E1]; **Manufacturer/product evidence**: Same absence of
  observed descriptors; **Interface evidence**: No exact interface number
  established; **Status of exact observed descriptors**: **STILL OPEN**
  normal-mode VID:PID, strings and interface numbers

### P1 — Posnet: actual observed family descriptors, but not Temo ONLINE

**SETTLED (observed):**
[DUG forum thread, post #1,
2019-12-27](https://forum.dug.net.pl/viewtopic.php?pid=326348#p326348),
titled _Drukarka Posnet Thermal XL2 i program DGCSSystem. Jak to pogodzić?_,
includes actual dmesg:

- `idVendor=1424, idProduct=10b0, bcdDevice=2.00`.
- `Product: Posnet D`, `Manufacturer: Posnet`, serial `4076F963`.
- `cdc_acm 3-3:1.0: ttyACM0: USB ACM device`.

[Post #3, 2019-12-28](https://forum.dug.net.pl/viewtopic.php?pid=326387#p326387)
contains udev attribute output confirming `bInterfaceNumber=00`, interface
class/subclass/protocol **02/02/01**, interface string
**`Posnet D Serial Port`**, device `bNumInterfaces=2`, manufacturer
**`Posnet`**, product/configuration **`Posnet D`**, and bcdDevice **0200**. This
is stronger than an INF for actual USB strings. It shows a shared family
descriptor rather than a marketing model name.

An independent
[Big Dot integrator
article](https://blog.bigdotsoftware.pl/posnet-server-konfiguracja-drukarki-fiskalnej/)
publishes udev output for **1424:10b0**, `ID_VENDOR=Posnet`, `ID_MODEL=Posnet`,
`ID_REVISION=0200`, `ID_USB_INTERFACE_NUM=00`, `ID_USB_DRIVER=cdc_acm`, and USB
interface classes `:020201:0a0000:`. The product reported there is **`Posnet`**,
not `Posnet D`; exact product-string equality would be brittle. This article
does not identify the precise printer model; its masked lsusb VID must not be
treated as observed 1424, but its explicit udev ID_VENDOR_ID is 1424.

Official INF corroboration: `USB/posnet/posnetCDC15v.utf8.inf:83-86,99-102`
match **1424:10b0 REV_0300 MI_00** and **REV_0200**, descriptions `:109,121-122`
are `POSNET S.A.` / `Posnet D Serial Port`. `:73,89,111` identify **1424:1015 as
Temo HS**. Neither proves the exact Temo ONLINE mapping.

[DeviceHunt's Posnet vendor
page](https://devicehunt.com/view/type/usb/vendor/1424)
lists **1001 Temo** and **1015 Temo HS** but no ONLINE qualifier or observed
descriptor strings. **CONTRADICTED:** treating that registry's
`1424:1001 = Temo` as proof of Temo ONLINE is not justified. Registry names are
lookup labels, not necessarily iProduct values.

### N1/N2 — Novitus: shared CDC driver IDs, no invented model strings

The official [HD II ONLINE](https://novitus.pl/wsparcie/pliki/hd-ii-online),
[DEON ONLINE](https://novitus.pl/wsparcie/pliki/deon-online), and
[BONO ONLINE](https://novitus.pl/wsparcie/pliki/bono-online) pages all link the
same online 64-bit driver URL in source #2. The
[POINT download page](https://point.novitus.pl/do-pobrania) links its own
matching archive.

Both INFs, `novitus.utf8.inf:26-32`, match:

- **USB\\VID_1A28&PID_2512&MI_00**, description
  **`Novitus Virtual serial Port A`**.
- **USB\\VID_1A28&PID_2512&MI_02**, description
  **`Novitus virtual serial Port B`**.
- Manufacturer **`Novitus`**, provider
  **`NOVITUS - Centrum Technologii Sprzedaży`** (`:81-86`).

**SETTLED:** A/B-to-communication-interface mapping in the supplied Windows
driver. **STILL OPEN:** whether every firmware/hardware revision of each model
uses exactly this PID, and its actual iManufacturer/iProduct/USB serial strings.
A shared INF does not establish unique model descriptors.

[The SZ USB registry](https://the-sz.com/products/usbid/index.php?v=0x1A28)
confirms **1a28 = NOVITUS SA**, but currently shows no useful per-model product
entry. Driver mirrors are weaker corroboration:
[DriverMax](https://www.drivermax.com/Novitus-virtual-serial-Port-B-Novitus-USB-VID-1A28-PID-2512-MI-02-1_3_6-2014-04-28-3899660-driver.htm)
independently records 1a28:2512 MI_02 for Port B, version 1.3.6. No need to
download or run their installers.

**N3 — Printer-side USB routing is configurable.**
[FakturaLight's specific Deon ONLINE
instructions](https://fakturalight.pl/fakturalight/instrukcja/novitus-deon-online)
set **COM3 (USB1)** to **1 PC**, all other ports to **0 (Brak)**. This is useful
operational evidence, but it does not establish that Linux ttyACM0 always means
USB1: enumeration indexes can change and are not a portable protocol-routing
contract.

[GoPOS's native-NOVITUS configuration
guide](https://help.gopos.pl/pl/articles/11864446-drukarki-fiskalne-novitus-konfiguracja)
explicitly covers all four requested models, chooses protocol **NOVITUS**, and
sets **USB1 to 1PC**, other ports to 0. Crucially,
[Fakturownia's field troubleshooting
guide](https://pomoc.fakturownia.pl/2350920-Bledy-z-wydrukiem-fiskalnym-i-konfiguracji-modulu-Paragony-pl),
section “Brak wydruku fizycznego paragonu…”, reports that **PC2 can return the
expected answers while no fiscal receipt is fiscalized**, with COM4 (USB2) as
its example of a port to disable. **SETTLED operational limitation:** port A/B
presence or a successful generic response cannot authorize fiscal work. Keep
candidates distinct and require the configured fiscal **PC/1PC** endpoint; do
not automatically fail over to the other endpoint or use a print as a discovery
test. **STILL OPEN:** an independently observed portable
USB-interface-to-printer-USB1/USB2 mapping.

**CONTRADICTED scope substitution:** **1a28:6010** really is Novitus **Bono E**,
but not proof of Bono ONLINE. The Linux upstream patch
[ee444609dbae8afee420c3243ce4c5f442efb622, reproduced in stable mailing
list](https://www.spinics.net/lists/stable/msg64640.html)
adds `NOVITUS_VID=0x1a28`, `NOVITUS_BONO_E_PID=0x6010` to
`drivers/usb/serial/ftdi_sio.c` and `ftdi_sio_ids.h`. Its 2014 date/model name
and FTDI driver establish legacy hardware, not online CDC revisions.

### E1/E2 — ELZAB: neither generic CDC nor legacy STM identity settles ONLINE IDs

Official
[download
page](https://www.elzab.com.pl/pl/strefa-dla-klienta/do-pobrania/programy),
locally `USB/elzab-programs-page.txt:122-141`, groups Mera/Cube/Zeta ONLINE with
`Usb_cdc_ser.zip`. The INF `USB/elzab/usb_cdc_ser.inf:22-32` matches
**USB\\Class_02&SubClass_02&Prot_01** and **Prot_00**, with **no VID/PID at
all**. `:118-121` gives provider **HCC Embedded**, description **CDC Device**,
service **USB Serial Driver**. These are generic Windows installation
properties, not printer-specific USB descriptors.

Nested `atm6124_cdc_signed/atm6124_cdc.inf:2,89-92` explicitly describes ATMEL
AT91SAM products and matches **03eb:6124**. **STILL OPEN:** normal operating
identity of the requested ONLINE printers. The mere presence of an Atmel
programming/CDC driver in a support ZIP does not establish a normal sales-mode
descriptor.

Legacy source #4 is linked separately under Zeta and D10 **without ONLINE**,
`USB/elzab-programs-page.txt:237-252`:

- `ANDROID/src/main/res/xml/device_filter.xml:3` filters **vendor-id 1155**,
  i.e. **0x0483** only; it does not filter a product ID.
- `ANDROID/src/main/java/com/hoho/android/usbserial/driver/UsbId.java:52-53`
  defines `VENDOR_ELZAB=0x0483`, `ELZAB_VIRTUAL_COM=0x5740`.
- [USB registry snapshot](https://usb-ids.gowdy.us/usb.ids), local
  `USB/linux-usb.ids:3531,3569`, assigns 0483 to **STMicroelectronics**, 5740 to
  generic **Virtual COM Port**.

**SETTLED:** vendor-supplied legacy Android support uses STM **0483:5740**, and
its attach filter allows all 0483 products. **CONTRADICTED:** calling 0483
ELZAB's exclusive VID, or treating the XML as ONLINE device evidence. **STILL
OPEN:** whether any of the ONLINE models also uses it. A source constant's name
is not enough.

[GoPOS's integrator
guide](https://help.gopos.pl/pl/articles/5089613-drukarki-fiskalne-elzab-konfiguracja)
covers Mera/Zeta/Cube USB with **Elzab STX**, direct USB A-B to Android POS, and
automatic discovery, but publishes **no VID/PID or USB strings**. It verifies a
real integration path, not an identifier allowlist.

## 2. `serialport` 4.10.1 exact platform behavior

### Version, feature gates and source

`Cargo.lock:5338-5353` resolves 4.10.1; dependency list has no libudev.
`Cargo.toml:134-135` explicitly uses `default-features = false` with comment
“sysfs enumeration avoids a libudev dependency.” Public `UsbPortInfo` has
VID/PID and optional serial/manufacturer/product (`SP/src/lib.rs:819-829`).
**Those fields do not require libudev** in this version. Interface requires
feature **usbportinfo-interface**, location requires **usbportinfo-location**
(`:830-837`); neither is requested in the workspace dependency.

Important portable-interface caveat: `SP/src/lib.rs:833-835` explicitly warns
that Linux/Windows generally report the **communication** interface, macOS may
report the **data** interface. Therefore even after enabling the feature, do not
portable-filter Novitus only on 0/2: macOS can report the paired data interface.
**The exact model-specific macOS 1/3 mapping remains unobserved**; the public
API warning is settled.

Context7 was consulted as requested for current library documentation; its
[available_ports
docs](https://github.com/serialport/serialport-rs/blob/main/_autodocs/api-reference/root-functions.md)
explicitly say ports are not guaranteed to exist or be available. Its broad
“less detail” description for no-libudev must not override the pinned source,
which reads all five core USB fields.

### Linux, actual Workshop configuration

Pinned `SP/src/posix/enumerate.rs:643-789`:

1. Scans `/sys/class/tty`, requires a `device` directory, canonicalizes its
   `subsystem`.
2. **pci → PciPort; pnp / serial-base / amba → Unknown; usb / usb-serial →
   UsbPort; unsupported subsystems → omitted** (`:692-708`).
3. USB reads parent device's `idVendor`, `idProduct`, optional `serial`,
   `product`, `manufacturer`; reads `bInterfaceNumber` only with the feature
   (`:716-739`). Native CDC ttyACM and USB-adapter ttyUSB therefore both become
   UsbPort when the expected sysfs ancestry/IDs are present. Names are not used
   to infer the physical type.
4. `rfcomm* → BluetoothPort` (`:648-654,751-752`).
5. Checks `/dev/<name>` **exists**, then pushes it; **no open, ioctl, UART type
   check or hardware-presence test** (`:771-785`).

**CONTRADICTED:** “available_ports already excludes all phantom ttyS entries.”
Old plain platform-backed serial8250 entries may be omitted because `platform`
is not an accepted subsystem, but serial-base-backed entries are explicitly
included as Unknown without any UART-presence check. Likewise PCI-backed
placeholders are not guaranteed functional merely because their class is
PciPort.

**CONTRADICTED:** “enable libudev to safely identify physical UARTs without
opening them.” The _other_ Linux branch `:613-625` explicitly opens every
parent-driver **serial8250** candidate at **9600 baud**, skipping it on failure.
That is a different build behavior and is undesirable as printer enumeration:
opening/configuring a port is not passive and may alter modem control. Do not
enable libudev just to inherit that check.

### macOS

Pinned `SP/src/posix/enumerate.rs:446-483,573-585`:

- USB ancestry via `IOUSBHostInterface`, falling back to legacy USB device class
  → **UsbPort**.
- Reads idVendor/idProduct (zero default on missing numeric property), USB
  Serial Number, USB Vendor Name, USB Product Name (strings optional).
- IOBluetoothSerialClient ancestry → **BluetoothPort**.
- Everything else returned by matching serial services → **PciPort**, not
  Unknown.
- Emits **both** IOCalloutDevice and IODialinDevice (`/dev/cu.*` and
  `/dev/tty.*`).

**SETTLED:** use `/dev/cu.*` for the UI/callout candidate to avoid duplicate
dial-in paths. **CONTRADICTED:** macOS PciPort proves actual PCI/native RS232
hardware; it is the fallback for all non-USB/non-Bluetooth serial services,
including any other virtual services that qualify.

### Windows

Pinned `SP/src/windows/enumerate.rs:171-193,410-430,590-635`:

- USB hardware/instance IDs are parsed, including USB composite parent and
  FTDIBUS forms; success → **UsbPort**.
- Manufacturer comes from **SPDRP_MFG**; product comes from
  **SPDRP_FRIENDLYNAME**, not guaranteed USB descriptor text (`:416-417`).
- Parsing failure or non-USB device → **Unknown**. This implementation has **no
  PciPort-producing path** and no BluetoothPort-producing path.
- SetupAPI devices with problem code other than Some(0) are skipped; LPT names
  are skipped.
- Registry-only SERIALCOMM additions are explicitly **Unknown**.

**SETTLED:** motherboard COM1/PCI serial-card COM ports that do not match USB
IDs are **Unknown** in this crate. Virtual COM ports and Bluetooth COM services
can also be Unknown. **CONTRADICTED:** retain only PciPort for Windows native
RS232; it would discard native ports. **STILL OPEN at crate abstraction:**
distinguish native UART versus arbitrary virtual/Bluetooth Unknown on Windows
using only SerialPortInfo: impossible because the source discards that
distinction. Additional SetupAPI ancestry/hardware-ID inspection or explicit
user configuration is required.

## 3. Passive Linux phantom-UART exclusion

**SETTLED kernel ABI:**
[sysfs-tty](https://www.kernel.org/doc/Documentation/ABI/testing/sysfs-tty)
documents `/sys/class/tty/ttyS<x>/type` as the TIOCGSERIAL tty type exposed
through sysfs, without ioctl.
[Linux v6.12 `serial_core.c`, `type_show` and
`uart_get_info`](https://github.com/torvalds/linux/blob/v6.12/drivers/tty/serial/serial_core.c)
implement it by reading the stored `uport->type`; no serial-device open or probe
occurs.
[v6.12 `include/uapi/linux/serial.h`, supported serial
types](https://github.com/torvalds/linux/blob/v6.12/include/uapi/linux/serial.h#L44-L65)
defines **PORT_UNKNOWN=0**, PORT_8250=1, etc.

[Same `serial_core.c`,
`uart_startup`](https://github.com/torvalds/linux/blob/v6.12/drivers/tty/serial/serial_core.c)
treats PORT_UNKNOWN as an uninitialized/non-operational port;
`uart_change_line_settings` explicitly describes unknown type as a port that
does not exist. Thus excluding `type == 0` is evidence-backed for kernel-unknown
placeholder slots. Nonzero type is **recognized UART**, not a
device-communication guarantee.

The DUG [post #3](https://forum.dug.net.pl/viewtopic.php?pid=326387#p326387) is
a real-world counterexample to a name-only filter: `/dev/ttyS5` points to a
serial8250 slot, while `/dev/ttyS3` was configured as a symlink to the real USB
ttyACM printer. Always classify the enumerated canonical device/sysfs ancestry
before applying native-UART logic; do not assume every ttyS-looking configured
path is native.

[INFERENCE / recommended policy] For automatic native-UART candidates, read and
parse the sysfs `type` as a decimal integer after canonical device resolution:
**0 → exclude automatic discovery; >0 → include as native-UART candidate, not
identified printer; missing/unreadable/malformed → unverified/manual-only, not
“phantom.”** Do not use open-at-9600 as a discovery check. Do not use IRQ>0,
nonzero legacy I/O address or a particular UART name as universal requirements:
serial-core supports MMIO, and `type` directly expresses recognition.

Limitation: this does not prove zero phantom ports for every UART/virtual driver
and OS. A nonzero-type emulator can still exist; a real physical UART may be
unrecognized until kernel/firmware configuration is fixed. Explicitly configured
ports should receive clear “unverified/unavailable” diagnostics rather than
being silently reclassified as a different printer.

## 4. Search coverage and remaining evidence requirements

Searched combinations of Novitus/Elzab/Posnet with lsusb, dmesg,
idVendor/idProduct, VID_/PID_, ttyACM, udev, device_filter.xml and usb-device,
including DUG, Elektroda, linux.org.ru, GitHub issues/repositories, DeviceHunt
and The SZ. Directly inspected the model download pages, all supplied INF
identities, legacy Android XML/UsbId constants, real DUG traces and Big Dot's
published udev output.
[GitHub issue query
Elzab+USB](https://api.github.com/search/issues?q=%22elzab%22+%22usb%22)
and
[Posnet+lsusb](https://api.github.com/search/issues?q=%22posnet%22+%22lsusb%22)
returned zero results at research time; Novitus+USB produced no descriptor
evidence. grep.app was unavailable (HTTP 429), so its absence of results is
**not** evidence of absence. The public repository-name search contained legacy
Elzab projects but yielded no model-specific observed ONLINE descriptor source.
Vendor binary JAR implementations were not decompiled; peer review of their
license flagged an express decompilation prohibition.

**STILL OPEN, precisely:** real normal-mode device descriptor captures
explicitly tied to Temo ONLINE, and each of Zeta/Mera/Cube ONLINE; real Novitus
per-model manufacturer/product strings; model/firmware revision-to-PID
inventory; exact observed macOS interface numbers. Resolving these requires
existing captures/source or a person obtaining passive OS descriptor data from
known hardware—not permission for the application to print, probe multiple
vendor protocols, or infer fiscal identity from a string. No support contact or
hardware action was taken.

## 5. Concrete filter rule table (recommendations, not printer identity guarantees)

Apply in order. “Candidate” means eligible for user selection/configured
read-only identification on the single Worker; it is never authorization to
perform fiscal work. These are [INFERENCE] implementation policies grounded in
the cited source behavior.

- **Order / platform — 1 / all**: **Condition**: No longer enumerated,
  inaccessible metadata, malformed fields; **Automatic-discovery decision**: Do
  not claim printer presence; preserve diagnostic/manual configuration;
  **Evidence / caveat**: available_ports has no existence/availability
  guarantee; Linux only checks path existence; SP POSIX 771-785, Windows 590-635
- **Order / platform — 2 / macOS**: **Condition**: `/dev/tty.*` dial-in
  duplicate; **Automatic-discovery decision**: Exclude duplicate; retain
  matching `/dev/cu.*` callout endpoint; **Evidence / caveat**: SP POSIX 573-585
  emits both
- **Order / platform — 3 / all**: **Condition**: `UsbPort`, VID:PID
  **1a28:2512**; **Automatic-discovery decision**: Include and rank as **Novitus
  family candidate**, show available A/B ports separately; fiscal use requires
  configured **PC/1PC**, not merely a responding endpoint; **Evidence /
  caveat**: Official INFs 26-32; GoPOS USB1=>1PC; Fakturownia PC2 false-success
  warning; SP lib.rs 833-835 warns macOS interface differs
- **Order / platform — 4 / all**: **Condition**: `UsbPort`, VID:PID
  **1424:10b0**; **Automatic-discovery decision**: Include and rank as **Posnet
  printer-family candidate**, not exact Temo ONLINE; **Evidence / caveat**:
  Observed DUG + official Posnet INF; 10b0 shared descriptors
- **Order / platform — 5 / all**: **Condition**: Other `UsbPort` with VID
  **1424** or **1a28**; **Automatic-discovery decision**: Optional
  lower-confidence vendor-family candidate; do not label supported ONLINE model
  solely from VID; **Evidence / caveat**: Registries/INFs also cover legacy
  hardware and Posnet cash registers; Bono E is 1a28:6010
- **Order / platform — 6 / all**: **Condition**: **0483:5740**, **03eb:6124**,
  or generic CDC strings/class only; **Automatic-discovery decision**: **No
  Elzab ONLINE brand assertion**. Keep as unidentified serial device/manual
  candidate if full RS232/CDC coverage is required; **Evidence / caveat**:
  Legacy STM sample and class-only INF do not establish ONLINE identity; generic
  controller/CDC identifiers
- **Order / platform — 7 / all**: **Condition**: Other USB serial adapter
  (FTDI/CP210x/CH34x/Prolific/etc.); **Automatic-discovery decision**: Preserve
  as **unidentified RS232 adapter/manual candidate**, never brand from adapter
  VID; **Evidence / caveat**: SP distinguishes USB transport only; a printer
  behind RS232 is not visible in USB descriptors
- **Order / platform — 8 / Linux**: **Condition**: Non-USB **PciPort or
  Unknown**, canonical serial-core UART with readable decimal sysfs `type > 0`;
  **Automatic-discovery decision**: Include as **native-UART candidate**, not a
  known printer; **Evidence / caveat**: SP pci/pnp/serial-base/amba mapping;
  Linux PORT_UNKNOWN ABI
- **Order / platform — 9 / Linux**: **Condition**: Canonical serial-core UART
  sysfs `type == 0`; **Automatic-discovery decision**: **Exclude from automatic
  discovery** as kernel-unrecognized slot; do not open it to test; **Evidence /
  caveat**: PORT_UNKNOWN=0; serial_core type_show/uart_get_info; no-libudev
  crate lacks this check
- **Order / platform — 10 / Linux**: **Condition**: Non-USB PciPort/Unknown but
  missing/unreadable/unparseable sysfs type, or unsupported ancestry;
  **Automatic-discovery decision**: **Manual/unverified only**; do not call it
  proven phantom or proven UART; **Evidence / caveat**: Crate's metadata cannot
  settle presence; missing metadata is not negative hardware proof
- **Order / platform — 11 / Linux/macOS**: **Condition**: **BluetoothPort**;
  **Automatic-discovery decision**: Exclude from USB/native-UART automatic list
  when Bluetooth is outside requested transport scope; **Evidence / caveat**: SP
  POSIX Linux rfcomm and macOS IOBluetooth ancestry branches
- **Order / platform — 12 / macOS**: **Condition**: **PciPort** after
  USB/Bluetooth handling; **Automatic-discovery decision**: **Manual/unverified
  non-USB candidate**, not proven native RS232; **Evidence / caveat**: This is
  macOS fallback, not actual PCI detection; SP POSIX 478-481
- **Order / platform — 13 / Windows**: **Condition**: **Unknown** COM port,
  including motherboard/PCI UART, registry-only or Bluetooth/virtual COM;
  **Automatic-discovery decision**: **Manual/unverified candidate**; use
  additional SetupAPI ancestry if product requires native-only auto-listing;
  **Evidence / caveat**: SP Windows 410-430 / 620-635; PciPort and BluetoothPort
  are never produced here
- **Order / platform — 14 / all**: **Condition**: Manufacturer/product strings;
  **Automatic-discovery decision**: Display as advisory metadata, never require
  exact string equality for inclusion or fiscal identity; **Evidence / caveat**:
  Observed Posnet `Posnet` versus `Posnet D`; Windows SPDRP properties differ
  from Linux/macOS USB descriptor sources
