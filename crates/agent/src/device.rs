//! Read-only brand discovery; unresolved operations never scan or switch brands.

use elzab::printer as elzab_printer;
use fiscal::{
    device::{Device, Vendor},
    driver::Driver,
    error::Error,
    link::{Connection, Link},
};
use novitus::printer as novitus_printer;
use posnet::printer as posnet_printer;
use std::sync::Arc;

/// A cached vendor is authoritative while blocked, but may be rediscovered otherwise.
pub fn detect(
    link: Arc<dyn Link>,
    connection: Connection,
    remembered: Option<Vendor>,
    may_switch: bool,
) -> Result<(Box<dyn Driver>, Device), Error> {
    let mut error = Error::Invalid("Nie znaleziono obsługiwanej drukarki fiskalnej.".to_owned());
    let mut denied = None;
    let mut inaccessible = false;
    if let Some(vendor) = remembered {
        match identify(link.as_ref(), &connection, vendor) {
            Ok(Some(device)) => return Ok((driver(link, connection, vendor), device)),
            Ok(None) => {},
            Err(failure) => {
                inaccessible = matches!(failure, Error::Open { .. });
                if failure.is_denied() {
                    denied = Some(connection.port.clone());
                }
                error = failure;
            },
        }
    }
    if !may_switch {
        return Err(error);
    }
    let candidates = link.candidates();
    if !inaccessible {
        let hint = candidates
            .iter()
            .find(|candidate| candidate.port == connection.port)
            .and_then(|candidate| candidate.hint);
        match probe(link.as_ref(), &connection, hint, remembered) {
            Ok((vendor, device)) => return Ok((driver(link, connection, vendor), device)),
            Err(failure) => {
                if failure.is_denied() && denied.is_none() {
                    denied = Some(connection.port.clone());
                }
                error = failure;
            },
        }
    }
    for candidate in candidates {
        if candidate.port == connection.port {
            continue;
        }
        let candidate_connection = Connection {
            port: candidate.port,
            baud_rate: connection.baud_rate,
        };
        match probe(link.as_ref(), &candidate_connection, candidate.hint, None) {
            Ok((vendor, device)) => {
                return Ok((driver(link, candidate_connection, vendor), device));
            },
            Err(failure) => {
                if failure.is_denied() && denied.is_none() {
                    denied = Some(candidate_connection.port);
                }
            },
        }
    }
    Err(denied.map_or(error, |port| {
        Error::Invalid(format!(
            "Brak uprawnień do portu {port}. Na Linuksie dodaj użytkownika do grupy dialout i \
         zaloguj się ponownie."
        ))
    }))
}

fn probe(
    link: &dyn Link,
    connection: &Connection,
    hint: Option<Vendor>,
    skipped: Option<Vendor>,
) -> Result<(Vendor, Device), Error> {
    let vendors = hint.map_or(
        [
            Some(Vendor::Posnet),
            Some(Vendor::Elzab),
            Some(Vendor::Novitus),
        ],
        |vendor| [Some(vendor), None, None],
    );
    let mut error = Error::Invalid("Nie znaleziono obsługiwanej drukarki fiskalnej.".to_owned());
    for vendor in vendors
        .into_iter()
        .flatten()
        .filter(|vendor| Some(*vendor) != skipped)
    {
        match identify(link, connection, vendor) {
            Ok(Some(device)) => return Ok((vendor, device)),
            Ok(None) => {},
            Err(failure @ Error::Open { .. }) => return Err(failure),
            Err(failure) => error = failure,
        }
    }
    Err(error)
}

fn identify(
    link: &dyn Link,
    connection: &Connection,
    vendor: Vendor,
) -> Result<Option<Device>, Error> {
    match vendor {
        Vendor::Posnet => posnet_printer::identify(link, connection),
        Vendor::Elzab => elzab_printer::identify(link, connection),
        Vendor::Novitus => novitus_printer::identify(link, connection),
    }
}

fn driver(link: Arc<dyn Link>, connection: Connection, vendor: Vendor) -> Box<dyn Driver> {
    match vendor {
        Vendor::Posnet => Box::new(posnet_printer::Printer::new(link, connection)),
        Vendor::Elzab => Box::new(elzab_printer::Printer::new(link, connection)),
        Vendor::Novitus => Box::new(novitus_printer::Printer::new(link, connection)),
    }
}

#[cfg(test)]
mod tests;
