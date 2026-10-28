//! Debug-only peers kept alive for the whole application lifetime: the three
//! printer simulators and a loopback Holuto filled with sample data for UI work.
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
        let novitus = NovitusSimulator::start();
        novitus.set_identity("NOVITUS HD II ONLINE", "3.50");
        let holuto = Holuto::new();
        sample_data(&holuto);
        Self {
            posnet: PosnetSimulator::start(),
            novitus,
            elzab: ElzabSimulator::start(),
            holuto,
        }
    }

    /// Points the settings at the chosen printer simulator and the sample Holuto,
    /// and starts processing on launch so the sample data shows at once.
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

/// Jobs in every state Paragony shows, plus orders for "+ Wystaw paragon".
fn sample_data(holuto: &Holuto) {
    let jobs = [
        (1041, "queued", "Anna Nowak", "KR98765", "Przegląd okresowy"),
        (1040, "done", "Łukasz Kowalski", "WA12345", "Wymiana oleju"),
        (
            1039,
            "failed",
            "Firma Transbud Sp. z o.o.",
            "PO44521",
            "Wymiana rozrządu",
        ),
        (
            1038,
            "expired",
            "Marek Wiśniewski",
            "GD7731K",
            "Geometria kół",
        ),
        (
            1037,
            "done",
            "Katarzyna Zielińska",
            "WW5521C",
            "Klimatyzacja",
        ),
        (
            1036,
            "voided",
            "Piotr Lewandowski",
            "LU22018",
            "Wymiana opon",
        ),
        (1035, "done", "Anna Nowak", "KR98765", "Wymiana oleju"),
    ];
    for (index, (number, state, customer, plate, service)) in (0_u8..).zip(jobs) {
        let id = Uuid::from_u128(number);
        let created = format!("2026-10-{:02}T{:02}:15:00Z", 27 - index / 3, 9 + index);
        let done = state == "done";
        let mut job = with(
            job_json(),
            &json!({
                "id": id, "orderId": Uuid::from_u128(number + 5000), "orderNumber": number,
                "state": state, "createdAt": created,
                "receiptNumber": done.then(|| format!("{}", 2100 + u32::from(index))),
                "issuedAt": done.then_some(&created),
                "result": (state == "failed").then_some("Nieprawidłowa stawka VAT dla pozycji 2"),
            }),
        );
        if let Some(name) = job.pointer_mut("/lines/0/name") {
            *name = json!(service);
        }
        holuto.insert(job);
        holuto.set_job_metadata(
            id,
            &json!({
                "customerName": customer, "vehiclePlate": plate,
                "issuedAt": done.then_some(&created), "source": "staff"
            }),
        );
    }
    // The fake queues every order as the `job_json` sale, so the amounts match it.
    let amount = job_json().get("total").cloned();
    holuto.documents(
        [
            (1042, "Tomasz Kamiński"),
            (1043, "Auto-Serwis Nowak"),
            (1044, "Joanna Wójcik"),
        ]
        .into_iter()
        .map(|(number, customer)| {
            json!({
                "kind": "sale", "orderId": Uuid::from_u128(number + 5000), "orderNumber": number,
                "allocationId": null, "amount": amount, "customerName": customer,
                "occurredAt": "2026-10-27T08:00:00Z", "dueAt": "2026-10-27T08:00:00Z",
                "latestAt": "2026-10-27T08:00:00Z", "isOverdue": false
            })
        })
        .collect(),
    );
}

#[cfg(test)]
mod tests;
