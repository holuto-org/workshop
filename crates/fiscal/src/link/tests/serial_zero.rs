use std::{
    io::{self, Read, Write},
    time::Duration,
};

use serialport::{ClearBuffer, DataBits, FlowControl, Parity, SerialPort, StopBits};

use crate::link::{Port, SerialStream};

/// Models the native serial API, including Windows' successful zero-byte timeout reads.
#[derive(Default)]
struct ZeroRead {
    timeout: Duration,
    reject_timeout: bool,
    read_error: Option<io::ErrorKind>,
    reads: usize,
    written: Vec<u8>,
    fail_write_after: Option<usize>,
}

impl Read for ZeroRead {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        self.reads += 1;
        self.read_error.map_or(Ok(0), |kind| Err(kind.into()))
    }
}
impl Write for ZeroRead {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let remaining = self.fail_write_after.unwrap_or(usize::MAX) - self.written.len();
        if remaining == 0 {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        let length = buf.len().min(2).min(remaining);
        self.written.extend_from_slice(&buf[..length]);
        Ok(length)
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
}
impl SerialPort for ZeroRead {
    fn name(&self) -> Option<String> {
        panic!("unexpected name query")
    }

    fn baud_rate(&self) -> serialport::Result<u32> {
        unsupported()
    }

    fn data_bits(&self) -> serialport::Result<DataBits> {
        unsupported()
    }

    fn flow_control(&self) -> serialport::Result<FlowControl> {
        unsupported()
    }

    fn parity(&self) -> serialport::Result<Parity> {
        unsupported()
    }

    fn stop_bits(&self) -> serialport::Result<StopBits> {
        unsupported()
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }

    fn set_baud_rate(&mut self, _baud_rate: u32) -> serialport::Result<()> {
        unsupported()
    }

    fn set_data_bits(&mut self, _data_bits: DataBits) -> serialport::Result<()> {
        unsupported()
    }

    fn set_flow_control(&mut self, _flow_control: FlowControl) -> serialport::Result<()> {
        unsupported()
    }

    fn set_parity(&mut self, _parity: Parity) -> serialport::Result<()> {
        unsupported()
    }

    fn set_stop_bits(&mut self, _stop_bits: StopBits) -> serialport::Result<()> {
        unsupported()
    }

    fn set_timeout(&mut self, timeout: Duration) -> serialport::Result<()> {
        if self.reject_timeout {
            return Err(serialport::Error::new(
                serialport::ErrorKind::Io(io::ErrorKind::InvalidInput),
                "test timeout rejected",
            ));
        }
        self.timeout = timeout;
        Ok(())
    }

    fn write_request_to_send(&mut self, _level: bool) -> serialport::Result<()> {
        unsupported()
    }

    fn write_data_terminal_ready(&mut self, _level: bool) -> serialport::Result<()> {
        unsupported()
    }

    fn read_clear_to_send(&mut self) -> serialport::Result<bool> {
        unsupported()
    }

    fn read_data_set_ready(&mut self) -> serialport::Result<bool> {
        unsupported()
    }

    fn read_ring_indicator(&mut self) -> serialport::Result<bool> {
        unsupported()
    }

    fn read_carrier_detect(&mut self) -> serialport::Result<bool> {
        unsupported()
    }

    fn bytes_to_read(&self) -> serialport::Result<u32> {
        unsupported()
    }

    fn bytes_to_write(&self) -> serialport::Result<u32> {
        unsupported()
    }

    fn clear(&self, _buffer_to_clear: ClearBuffer) -> serialport::Result<()> {
        unsupported()
    }

    fn try_clone(&self) -> serialport::Result<Box<dyn SerialPort>> {
        unsupported()
    }

    fn set_break(&self) -> serialport::Result<()> {
        unsupported()
    }

    fn clear_break(&self) -> serialport::Result<()> {
        unsupported()
    }
}

fn unsupported<T>() -> serialport::Result<T> {
    Err(serialport::Error::new(
        serialport::ErrorKind::Unknown,
        "unsupported test serial setting",
    ))
}

#[test]
fn zero_byte_serial_reads_are_timeouts_not_disconnects_or_empty_replies() {
    let mut port = SerialStream(ZeroRead::default());
    assert_eq!(
        Port::read(&mut port, &mut [0; 16], Duration::from_millis(30))
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut
    );
    assert_eq!(port.0.timeout, Duration::from_millis(30));
}

#[test]
fn native_read_errors_are_preserved_instead_of_becoming_timeouts() {
    for kind in [
        io::ErrorKind::BrokenPipe,
        io::ErrorKind::Interrupted,
        io::ErrorKind::WouldBlock,
    ] {
        let mut port = SerialStream(ZeroRead {
            read_error: Some(kind),
            ..ZeroRead::default()
        });
        assert_eq!(
            Port::read(&mut port, &mut [0; 1], Duration::from_millis(17))
                .unwrap_err()
                .kind(),
            kind
        );
        assert_eq!(port.0.timeout, Duration::from_millis(17));
    }
}

#[test]
fn writing_completes_partial_native_writes_without_flushing() {
    let mut port = SerialStream(ZeroRead::default());
    Port::write_all(&mut port, b"abcdef", Duration::from_millis(31)).unwrap();
    assert_eq!(port.0.written, b"abcdef");
    assert_eq!(port.0.timeout, Duration::from_millis(31));
}

#[test]
fn a_native_write_failure_preserves_the_partial_delivery_and_error() {
    let mut port = SerialStream(ZeroRead {
        fail_write_after: Some(2),
        ..ZeroRead::default()
    });
    assert_eq!(
        Port::write_all(&mut port, b"abcdef", Duration::from_millis(31))
            .unwrap_err()
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(port.0.written, b"ab");
    assert_eq!(port.0.timeout, Duration::from_millis(31));
}

#[test]
fn rejected_native_timeouts_prevent_the_read_or_write() {
    let mut port = SerialStream(ZeroRead {
        reject_timeout: true,
        read_error: Some(io::ErrorKind::BrokenPipe),
        ..ZeroRead::default()
    });
    assert_eq!(
        Port::read(&mut port, &mut [0; 1], Duration::from_millis(1))
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        Port::write_all(&mut port, b"abc", Duration::from_millis(1))
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(port.0.written, b"");
    assert_eq!(port.0.reads, 0);
    assert_eq!(port.0.timeout, Duration::ZERO);
}
