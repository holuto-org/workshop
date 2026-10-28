//! Debug-only peers kept alive for the whole application lifetime: the three
//! printer simulators and a loopback Holuto with sample jobs in its queue.
//!
//! A simulator run starts processing on launch, so the samples visibly print.
use agent::{
    settings::Settings,
    testing::{Holuto, job_json, with},
};
use elzab::simulator::Simulator as ElzabSimulator;
use fiscal::{device::Vendor, link::Link};
use novitus::simulator::Simulator as NovitusSimulator;
use posnet::simulator::Simulator as PosnetSimulator;
use serde_json::json;
use std::{io, sync::Arc};
use uuid::Uuid;

pub(super) struct Simulators {
    posnet: PosnetSimulator,
    novitus: NovitusSimulator,
    elzab: ElzabSimulator,
    holuto: Holuto,
}
impl Simulators {
    pub(super) fn start() -> Self {
        // Brands that print no company footer need the company in their header.
        let novitus = NovitusSimulator::start();
        novitus.set_identity("NOVITUS HD II ONLINE", "3.50");
        novitus.set_header(COMPANY);
        let elzab = ElzabSimulator::start();
        elzab.set_header(COMPANY);
        let holuto = Holuto::new();
        queue_samples(&holuto);
        Self {
            posnet: PosnetSimulator::start(),
            novitus,
            elzab,
            holuto,
        }
    }

    /// Points the settings at the chosen printer simulator and the sample Holuto,
    /// and starts processing on launch so the queued samples print at once.
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
        self.holuto.url().clone_into(&mut settings.api_url);
        settings.token = self.holuto.token().into();
        settings.start_on_launch = true;
        Ok((settings, link))
    }
}

/// The sample jobs: a sale and an advance.
const SAMPLE_JOBS: [Uuid; 2] = [Uuid::from_u128(1041), Uuid::from_u128(1042)];
/// The workshop's Holuto company profile, as printed by every simulated brand.
const COMPANY: &str = "Warsztat Kowalski";

/// Queues the sample jobs as staff would queue them in Holuto.
fn queue_samples(holuto: &Holuto) {
    let [sale, advance] = SAMPLE_JOBS;
    let sale = with(
        job_json(),
        &json!({
            "id": sale, "orderId": Uuid::from_u128(6041), "orderNumber": 1041,
            "companyName": COMPANY
        }),
    );
    // ASCII names print unchanged on every simulated brand.
    let advance = with(
        job_json(),
        &json!({
            "id": advance, "orderId": Uuid::from_u128(6042), "orderNumber": 1042,
            "companyName": COMPANY, "kind": "advance", "allocationId": Uuid::from_u128(7042),
            "total": "300.00", "payments": [{"form": "card", "amount": "300.00"}],
            "lines": [
                {"name": "Zaliczka 23%", "quantity": "1", "unitPrice": "300.00",
                 "total": "300.00", "vatRate": "23", "vatLetter": "A"}
            ]
        }),
    );
    for job in [sale, advance] {
        let _id = holuto.enqueue(job);
    }
}

#[cfg(test)]
mod tests;
