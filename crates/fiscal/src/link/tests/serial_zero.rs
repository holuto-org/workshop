use std::{
    io::{self, Read, Write},
    time::Duration,
};

use serialport::{ClearBuffer, DataBits, FlowControl, Parity, SerialPort, StopBits};

use crate::link::{Port, SerialStream};

/// The Windows unique_number API returns successful zero-byte reads for expired timeouts.
struct ZeroRead {
    timeout: Duration,
}

impl Read for ZeroRead {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Ok(0)
    }
}
impl Write for ZeroRead {
    fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::Unsupported.into())
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
        "unsupported test unique_number setting",
    ))
}

#[test]
fn zero_byte_serial_reads_are_timeouts_not_disconnects_or_empty_replies() {
    let mut port = SerialStream(ZeroRead {
        timeout: Duration::ZERO,
    });
    assert_eq!(
        Port::read(&mut port, &mut [0; 16], Duration::from_millis(30))
            .unwrap_err()
            .kind(),
        io::ErrorKind::TimedOut
    );
    assert_eq!(port.0.timeout, Duration::from_millis(30));
}
