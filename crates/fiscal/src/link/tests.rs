use serialport::{SerialPortInfo, SerialPortType, UsbPortInfo};

use super::filter_serial_ports;

mod serial_zero;
#[test]
fn macos_filters_carrier_waiting_duplicates_and_keeps_usb_first_on_every_os() {
    let ports = vec![
        SerialPortInfo {
            port_name: "/dev/tty.Bluetooth".to_owned(),
            port_type: SerialPortType::BluetoothPort,
        },
        SerialPortInfo {
            port_name: "COM1".to_owned(),
            port_type: SerialPortType::Unknown,
        },
        SerialPortInfo {
            port_name: "/dev/tty.usbmodem".to_owned(),
            port_type: SerialPortType::UsbPort(UsbPortInfo {
                vid: 1,
                pid: 2,
                serial_number: None,
                manufacturer: None,
                product: None,
            }),
        },
        SerialPortInfo {
            port_name: "/dev/cu.usbmodem".to_owned(),
            port_type: SerialPortType::UsbPort(UsbPortInfo {
                vid: 1,
                pid: 2,
                serial_number: None,
                manufacturer: None,
                product: None,
            }),
        },
    ];
    assert_eq!(
        filter_serial_ports(ports.clone(), true),
        ["/dev/cu.usbmodem", "COM1"]
    );
    assert_eq!(
        filter_serial_ports(ports, false),
        [
            "/dev/tty.usbmodem",
            "/dev/cu.usbmodem",
            "/dev/tty.Bluetooth",
            "COM1"
        ]
    );
}
