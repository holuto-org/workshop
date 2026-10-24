use std::{
    collections::VecDeque,
    sync::{Mutex, mpsc},
    thread,
    time::Duration,
};

use posnet::{
    printer::{Printer, Status},
    session::Connection,
    simulator::{SimReport, Simulator},
};
use serde_json::json;
use uuid::Uuid;

use super::{Command, Event, Message, Pager, Session, Tone};
use crate::{
    agent::Agent,
    api::model::{Outcome, Page},
    documents::Document,
    journal::{Journal, Operation},
    testing::{FakeApi, job, state_dir, unheard},
};

fn document(number: i64) -> Document {
    serde_json::from_value(json!({
        "kind": "sale",
        "orderId": Uuid::from_u128(u128::try_from(number).unwrap()),
        "orderNumber": number,
        "allocationId": null,
        "amount": "10.00",
        "customerName": null,
        "occurredAt": "2026-10-16T10:00:00Z"
    }))
    .unwrap()
}

fn page(items: Vec<Document>, next: Option<&str>) -> Page {
    Page {
        items,
        next_cursor: next.map(str::to_owned),
    }
}

fn shown(event: Event) -> (Vec<i64>, usize, bool) {
    let Event::Documents {
        items,
        page,
        has_next,
    } = event
    else {
        panic!("{event:?}")
    };
    (
        items.iter().map(|item| item.order_number).collect(),
        page,
        has_next,
    )
}

/// Holuto serving these documents pages, keyed by the cursor that opens each.
fn serving(pages: Vec<(Option<&str>, Page)>) -> FakeApi {
    let mut api = FakeApi::new(vec![]);
    api.pages = pages
        .into_iter()
        .map(|(cursor, page)| (cursor.map(str::to_owned), page))
        .collect();
    api
}

/// The cursors Holuto was asked for, in order.
fn requested(api: &FakeApi) -> Vec<Option<&str>> {
    api.cursors.iter().map(Option::as_deref).collect()
}

#[test]
fn each_poll_fetches_only_the_shown_page_and_buttons_page_through() {
    let mut api = serving(vec![
        (None, page(vec![document(1)], Some("after-1"))),
        (Some("after-1"), page(vec![document(2)], None)),
    ]);
    let mut pager = Pager::default();
    assert_eq!(shown(pager.refresh(&mut api).unwrap()), (vec![1], 1, true));
    assert_eq!(
        shown(pager.refresh(&mut api).unwrap()),
        (vec![1], 1, true),
        "a later poll refreshes the same page"
    );
    pager.next();
    assert_eq!(shown(pager.refresh(&mut api).unwrap()), (vec![2], 2, false));
    pager.next();
    assert_eq!(
        shown(pager.refresh(&mut api).unwrap()),
        (vec![2], 2, false),
        "no next page: stays put"
    );
    pager.previous();
    assert_eq!(shown(pager.refresh(&mut api).unwrap()), (vec![1], 1, true));
    assert_eq!(
        requested(&api),
        [None, None, Some("after-1"), Some("after-1"), None]
    );
}

#[test]
fn a_failed_later_page_keeps_the_shown_page_and_its_cursor() {
    let mut api = serving(vec![(None, page(vec![document(1)], Some("after-1")))]);
    let mut pager = Pager::default();
    let _first = pager.refresh(&mut api).unwrap();
    pager.next();
    assert!(pager.refresh(&mut api).is_err());
    assert_eq!(
        (pager.pages.clone(), pager.next.as_deref()),
        (vec![None], Some("after-1"))
    );
    assert_eq!(
        shown(pager.refresh(&mut api).unwrap()),
        (vec![1], 1, true),
        "not a half-loaded page 2"
    );
    assert_eq!(requested(&api), [None, Some("after-1"), None]);
}

#[test]
fn an_emptied_later_page_returns_to_the_first() {
    let mut api = serving(vec![
        (None, page(vec![document(1)], Some("after-1"))),
        (Some("after-1"), page(vec![], None)),
    ]);
    let mut pager = Pager::default();
    let _first = pager.refresh(&mut api).unwrap();
    pager.next();
    assert_eq!(shown(pager.refresh(&mut api).unwrap()), (vec![1], 1, true));
    assert_eq!(requested(&api), [None, Some("after-1"), None]);
}

/// The worker's loop, with Holuto replaced, on its own thread as in the app.
#[test]
fn receipts_and_reports_run_on_one_thread_and_stop_waits_for_the_current_print() {
    let sim = Simulator::start();
    let (_temporary, dir) = state_dir();
    let mut api = serving(vec![(None, page(vec![], None))]);
    api.jobs.push_back(job());
    let events = Mutex::new(Vec::new());
    let sink = |event: Event| events.lock().unwrap().push(event);
    let (messages, inbox) = mpsc::channel();
    let hold = sim.hold("trend");
    messages
        .send(Message::Command(Command::DailyReport))
        .unwrap();
    let (simulator, state, events_sink) = (&sim, &dir, &sink);
    thread::scope(|scope| {
        let worker = scope.spawn(move || {
            let connection = Connection {
                port: simulator.address().to_owned(),
                baud_rate: 9600,
            };
            let mut agent = Agent::new(
                Printer::new(simulator.link(), connection),
                api,
                state,
                &unheard,
            );
            let _notices = agent.describe(printer_status(simulator)).unwrap();
            let pager = Pager::default();
            Session {
                agent,
                pager,
                queue: VecDeque::new(),
                sink: events_sink,
            }
            .run(&inbox)
        });
        assert!(
            hold.reached(Duration::from_secs(5)),
            "the queued job reached the printer"
        );
        let reports: Vec<String> = sim
            .reports()
            .into_iter()
            .map(|SimReport { command, .. }| command)
            .collect();
        assert_eq!(
            reports,
            ["dailyrep"],
            "the report ran before the receipt, never interleaved"
        );
        messages.send(Message::Stop).unwrap();
        assert!(!worker.is_finished(), "stop waits for the current print");
        hold.release();
        worker.join().unwrap().unwrap();
    });
    assert_eq!(sim.receipts().len(), 1);
    let events = events.into_inner().unwrap();
    assert!(events.iter().any(report_printed));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::HistoryChanged))
    );
    let Some(Operation::Receipt(operation)) = Journal::new(&dir).load().unwrap() else {
        panic!("the receipt is journaled");
    };
    assert_eq!(
        operation.result_form().map(|form| form.state),
        Some(Outcome::Done)
    );
}

fn printer_status(sim: &Simulator) -> Status {
    let connection = Connection {
        port: sim.address().to_owned(),
        baud_rate: 9600,
    };
    Printer::new(sim.link(), connection).probe().unwrap()
}

fn report_printed(event: &Event) -> bool {
    matches!(event, Event::Status { tone: Tone::Success, text } if text.contains("Raport"))
}
