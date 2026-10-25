//! TEMPORARY probe; removed before asking.
use std::{io, sync::Arc};

use fiscal::{
    error::Error,
    link::{Candidate, Connection, Link, Port},
};

#[derive(Debug)]
struct NoPrinter;

impl Link for NoPrinter {
    fn open(&self, port: &str, _baud_rate: u32) -> Result<Box<dyn Port>, Error> {
        Err(Error::Open {
            port: port.to_owned(),
            source: io::ErrorKind::NotFound.into(),
        })
    }

    fn candidates(&self) -> Vec<Candidate> {
        Vec::new()
    }
}

#[test]
fn probe_auto_port_without_printer() {
    let result = agent::device::detect(
        Arc::new(NoPrinter),
        Connection {
            port: String::new(),
            baud_rate: 9600,
        },
        None,
        true,
    );
    let error = result.err().unwrap();
    println!("PROBE error variant: {error:?}");
    println!("PROBE operator text: {error}");
}
