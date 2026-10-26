//! Debug-only peers kept alive for the whole application lifetime.
use agent::settings::Settings;
use elzab::simulator::Simulator as ElzabSimulator;
use fiscal::{device::Vendor, link::Link};
use novitus::simulator::Simulator as NovitusSimulator;
use posnet::simulator::Simulator as PosnetSimulator;
use std::{io, sync::Arc};

pub(super) struct Simulators {
    posnet: PosnetSimulator,
    novitus: NovitusSimulator,
    elzab: ElzabSimulator,
}
impl Simulators {
    pub(super) fn start() -> Self {
        let novitus = NovitusSimulator::start();
        novitus.set_identity("NOVITUS HD II ONLINE", "3.50");
        Self {
            posnet: PosnetSimulator::start(),
            novitus,
            elzab: ElzabSimulator::start(),
        }
    }

    pub(super) fn connection(
        &self,
        mut settings: Settings,
        choice: &str,
    ) -> io::Result<(Settings, Arc<dyn Link>)> {
        let (port, vendor, link) = match choice {
            "posnet" => (self.posnet.address(), Vendor::Posnet, self.posnet.link()),
            "novitus" => (self.novitus.address(), Vendor::Novitus, self.novitus.link()),
            "elzab" => (self.elzab.address(), Vendor::Elzab, self.elzab.link()),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "WORKSHOP_SIMULATOR: oczekiwano posnet, novitus lub elzab.",
                ));
            },
        };
        port.clone_into(&mut settings.port);
        settings.vendor = Some(vendor);
        Ok((settings, link))
    }
}

#[cfg(test)]
mod tests;
