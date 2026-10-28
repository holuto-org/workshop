use serialport::{SerialPortInfo, SerialPortType, UsbPortInfo};

use super::{Candidate, Os, filter};
use crate::device::Vendor;

mod serial_zero;

fn port(name: &str, port_type: SerialPortType) -> SerialPortInfo {
    SerialPortInfo {
        port_name: name.into(),
        port_type,
    }
}

fn usb(name: &str, vid: u16, pid: u16) -> SerialPortInfo {
    port(
        name,
        SerialPortType::UsbPort(UsbPortInfo {
            vid,
            pid,
            serial_number: None,
            manufacturer: None,
            product: None,
        }),
    )
}

fn candidate(name: &str, hint: Option<Vendor>) -> Candidate {
    Candidate {
        port: name.into(),
        hint,
    }
}

#[test]
fn keeps_printer_usb_ids_with_brand_hints() {
    for os in [Os::Linux, Os::Windows, Os::Macos] {
        assert_eq!(
            filter(
                vec![
                    usb("a-posnet", 0x1424, 0x10B0),
                    usb("b-posnet-family", 0x1424, 0xFFFF),
                    usb("c-novitus-a", 0x1A28, 0x2512),
                    usb("e-elzab", 0x0483, 0x5740),
                ],
                os,
                &|_| panic!("USB candidates must not read native UART sysfs metadata"),
            ),
            vec![
                candidate("a-posnet", Some(Vendor::Posnet)),
                candidate("b-posnet-family", Some(Vendor::Posnet)),
                candidate("c-novitus-a", Some(Vendor::Novitus)),
                candidate("e-elzab", Some(Vendor::Elzab)),
            ]
        );
    }
}

#[test]
fn keeps_rs232_bridges_and_native_uarts_without_hint() {
    for os in [Os::Linux, Os::Windows, Os::Macos] {
        assert_eq!(
            filter(
                vec![
                    usb("a-ftdi", 0x0403, 0xFFFF),
                    usb("b-prolific", 0x067B, 0x0000),
                    usb("c-wch", 0x1A86, 0x7523),
                    usb("d-silabs", 0x10C4, 0xEA60),
                    port(
                        "native",
                        match os {
                            Os::Windows => SerialPortType::Unknown,
                            Os::Linux | Os::Macos => SerialPortType::PciPort,
                        },
                    ),
                ],
                os,
                &|name| {
                    assert_eq!(name, "native");
                    Some(1)
                },
            ),
            vec![
                candidate("a-ftdi", None),
                candidate("b-prolific", None),
                candidate("c-wch", None),
                candidate("d-silabs", None),
                candidate("native", None),
            ]
        );
    }
}

#[test]
fn drops_bluetooth_other_usb_and_phantom_linux_uarts() {
    assert_eq!(
        filter(
            vec![
                port("/dev/rfcomm0", SerialPortType::BluetoothPort),
                usb("/dev/ttyACM0", 0x1234, 0x5678),
                usb("/dev/ttyACM1", 0x1A28, 0x6010),
                usb("/dev/ttyACM2", 0x0483, 0x5741),
                usb("/dev/ttyACM3", 0x03EB, 0x6124),
                port("/dev/ttyS0", SerialPortType::PciPort),
                port("/dev/ttyS1", SerialPortType::Unknown),
            ],
            Os::Linux,
            &|_| Some(0),
        ),
        vec![]
    );
}

#[test]
fn linux_keeps_known_and_unreadable_native_uarts() {
    assert_eq!(
        filter(
            vec![
                port("/dev/ttyS2", SerialPortType::Unknown),
                port("/dev/ttyS1", SerialPortType::Unknown),
                port("/dev/ttyS0", SerialPortType::PciPort),
            ],
            Os::Linux,
            &|name| match name {
                "/dev/ttyS0" => Some(1),
                "/dev/ttyS1" => Some(u32::MAX),
                _ => None,
            },
        ),
        vec![
            candidate("/dev/ttyS0", None),
            candidate("/dev/ttyS1", None),
            candidate("/dev/ttyS2", None),
        ]
    );
}

#[test]
fn windows_keeps_unknown_com_ports_without_asserting_physical_identity() {
    assert_eq!(
        filter(
            vec![
                port("COM3", SerialPortType::Unknown),
                port("COM1", SerialPortType::Unknown),
                port("COM2", SerialPortType::PciPort),
                port("COM4", SerialPortType::BluetoothPort),
                usb("COM5", 0x1234, 0x5678),
            ],
            Os::Windows,
            &|_| panic!("Windows enumeration must not read Linux sysfs"),
        ),
        vec![candidate("COM1", None), candidate("COM3", None)]
    );
}

#[test]
fn macos_keeps_callout_drops_dialin_duplicates() {
    assert_eq!(
        filter(
            vec![
                usb("/dev/tty.usbmodem", 0x1424, 0x10B0),
                usb("/dev/cu.usbmodem", 0x1424, 0x10B0),
                port("/dev/tty.serial", SerialPortType::PciPort),
                port("/dev/cu.serial", SerialPortType::PciPort),
                port("/dev/cu.unknown", SerialPortType::Unknown),
                port("/dev/cu.Bluetooth", SerialPortType::BluetoothPort),
            ],
            Os::Macos,
            &|_| panic!("macOS enumeration must not read Linux sysfs"),
        ),
        vec![
            candidate("/dev/cu.usbmodem", Some(Vendor::Posnet)),
            candidate("/dev/cu.serial", None),
        ]
    );
}

#[test]
fn orders_hinted_then_bridges_then_native() {
    assert_eq!(
        filter(
            vec![
                port("a-native", SerialPortType::PciPort),
                usb("z-bridge", 0x0403, 0x6001),
                usb("z-hinted", 0x1424, 0x10B0),
                usb("a-bridge", 0x10C4, 0xEA60),
                port("z-native", SerialPortType::Unknown),
                usb("a-hinted", 0x1A28, 0x2512),
            ],
            Os::Linux,
            &|_| None,
        ),
        vec![
            candidate("a-hinted", Some(Vendor::Novitus)),
            candidate("z-hinted", Some(Vendor::Posnet)),
            candidate("a-bridge", None),
            candidate("z-bridge", None),
            candidate("a-native", None),
            candidate("z-native", None),
        ]
    );
}

#[test]
fn usb_descriptions_and_serial_numbers_never_become_verified_identity() {
    let described = |name: &str, vid, pid| {
        port(
            name,
            SerialPortType::UsbPort(UsbPortInfo {
                vid,
                pid,
                serial_number: Some("ABC1234567890".to_owned()),
                manufacturer: Some("Posnet Novitus Elzab".to_owned()),
                product: Some("Temo Online".to_owned()),
            }),
        )
    };
    for os in [Os::Linux, Os::Windows, Os::Macos] {
        assert_eq!(
            filter(
                vec![
                    described("unknown", 0x1234, 0x5678),
                    described("bridge", 0x0403, 0x6001),
                    described("hint", 0x0483, 0x5740),
                ],
                os,
                &|_| panic!("USB metadata must never query native UARTs"),
            ),
            vec![
                candidate("hint", Some(Vendor::Elzab)),
                candidate("bridge", None)
            ],
        );
    }
}
