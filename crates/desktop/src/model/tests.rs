#![expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture and UI setup"
)]

use std::{cell::RefCell, error::Error, rc::Rc};

use super::*;
use agent::documents::ReceiptKind;
use jiff::Timestamp;
use posnet::money::Grosze;
use slint::{
    ComponentHandle as _, LogicalPosition, LogicalSize, Model as _, PlatformError,
    platform::{
        Platform, PointerEventButton, WindowAdapter, WindowEvent, set_platform,
        software_renderer::{MinimalSoftwareWindow, RepaintBufferType},
    },
};
use uuid::Uuid;

fn document() -> Document {
    Document {
        kind: ReceiptKind::Sale,
        order_id: Uuid::nil(),
        order_number: 42,
        allocation_id: None,
        amount: 25_000,
        customer_name: Some("Jan Kowalski".into()),
        occurred_at: Timestamp::UNIX_EPOCH,
    }
}

/// A windowless platform, so tests drive `AppWindow` properties without a display server.
struct Headless;

impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
    }
}

/// Slint keeps one platform per thread and every test runs on its own thread.
fn window() -> Result<AppWindow, Box<dyn Error>> {
    set_platform(Box::new(Headless))?;
    Ok(AppWindow::new()?)
}

#[test]
fn controls_follow_connection_selection_readiness_and_blocking() -> Result<(), Box<dyn Error>> {
    let window = window()?;
    let mut model = Model::new(Settings::default());
    model.set_documents(vec![document()], 1, false);
    model.selected = Some(0);
    model.sync(&window);
    assert!(
        !window.get_queue_enabled(),
        "Disconnected operators cannot queue"
    );
    assert!(
        !window.get_reports_enabled(),
        "Disconnected operators cannot report"
    );
    model.connected = true;
    model.running = true;
    model.page = 2;
    model.has_next = true;
    model.sync(&window);
    assert!(
        window.get_queue_enabled(),
        "A selected connected document can be queued"
    );
    assert!(
        !window.get_reports_enabled(),
        "Reports require a ready device"
    );
    assert!(
        window.get_previous_enabled() && window.get_next_enabled(),
        "Pager follows the published page"
    );
    model.device = Some(Status {
        name: String::new(),
        version: String::new(),
        serial: String::new(),
        problems: vec![],
        vat: vec![],
    });
    model.sync(&window);
    assert!(window.get_reports_enabled(), "Ready device enables reports");
    model.blocked = true;
    model.sync(&window);
    assert!(
        !window.get_queue_enabled() && !window.get_reports_enabled(),
        "Unresolved operations block printing"
    );
    model.page = 1;
    model.has_next = false;
    model.sync(&window);
    assert!(
        !window.get_previous_enabled() && !window.get_next_enabled(),
        "First and last page disable navigation"
    );
    Ok(())
}

#[test]
fn queue_confirmation_captures_document_and_validated_nip() {
    let mut model = Model::new(Settings::default());
    model.connected = true;
    model.set_documents(vec![document()], 1, false);
    model.selected = Some(0);
    model.nip = "123".into();
    model.request(Action::QueueReceipt);
    assert_eq!(model.message, "NIP nabywcy musi zawierać 10 cyfr.");
    assert!(
        model.pending.is_none(),
        "Invalid NIP must not reach confirmation"
    );
    model.nip = "1234567890".into();
    model.request(Action::QueueReceipt);
    let mut commands = Vec::new();
    model.confirm(true, |command| commands.push(command));
    assert_eq!(
        commands,
        vec![Command::Queue {
            document: document(),
            buyer_nip: BuyerNip::parse("1234567890").ok()
        }]
    );
}

fn ready_model() -> Model {
    let mut model = Model::new(Settings::default());
    model.connected = true;
    model.device = Some(Status {
        name: String::new(),
        version: String::new(),
        serial: String::new(),
        problems: vec![],
        vat: vec![],
    });
    model
}

#[test]
fn report_commands_and_date_order() -> Result<(), Box<dyn Error>> {
    let mut model = ready_model();
    let mut commands = Vec::new();
    model.request(Action::DailyReport);
    model.confirm(true, |command| commands.push(command));
    model.request(Action::MonthlyReport);
    model.confirm(true, |command| commands.push(command));
    let month = model.months.first().ok_or("Missing previous month")?;
    assert_eq!(
        commands,
        vec![
            Command::DailyReport,
            Command::MonthlyReport {
                year: month.year(),
                month: month.month()
            }
        ]
    );
    model.from = "20.10.2025".into();
    model.to = "19.10.2025".into();
    model.request(Action::PeriodicReport);
    assert_eq!(
        model.message,
        "Data początkowa nie może być późniejsza niż końcowa."
    );
    assert!(
        model.pending.is_none(),
        "Reversed dates cannot be confirmed"
    );
    model.to = "21.10.2025".into();
    model.summary = true;
    model.request(Action::PeriodicReport);
    model.confirm(true, |command| commands.push(command));
    assert_eq!(
        commands.last(),
        Some(&Command::PeriodicReport {
            from: Date::new(2025, 10, 20)?,
            to: Date::new(2025, 10, 21)?,
            summary: true
        })
    );
    Ok(())
}

#[test]
fn refresh_retains_identity_not_old_row_index() {
    let mut model = Model::new(Settings::default());
    let selected = document();
    let mut other = document();
    other.order_id = Uuid::from_u128(1);
    model.set_documents(vec![selected.clone(), other.clone()], 1, false);
    model.selected = Some(0);
    model.set_documents(vec![other, selected], 2, false);
    assert_eq!(model.selected, Some(1));
    model.set_documents(vec![], 1, false);
    assert_eq!(model.selected, None);
}

#[test]
fn window_rolls_a_stale_month_list_and_keeps_the_choice() -> Result<(), Box<dyn Error>> {
    let window = window()?;
    let mut model = ready_model();
    // The list as built two months ago, shown with its third month chosen.
    model.months = months_before(Zoned::now().date().checked_sub(2.months())?);
    window.set_months(strings(model.months.iter().copied().map(month_label)));
    model.month_index = 2;
    let chosen = model.months[2];
    model.sync(&window);
    model.capture(&window);
    model.sync(&window);
    let ended = Zoned::now()
        .date()
        .first_of_month()
        .checked_sub(1.month())?;
    assert_eq!(
        window.get_months().row_data(0),
        Some(month_label(ended).into()),
        "The month that just ended is offered first"
    );
    let index = usize::try_from(window.get_month_index())?;
    assert_eq!(
        window.get_months().row_data(index),
        Some(month_label(chosen).into()),
        "The operator's month stays selected"
    );
    let mut commands = Vec::new();
    model.request(Action::MonthlyReport);
    model.confirm(true, |command| commands.push(command));
    assert_eq!(
        commands,
        vec![Command::MonthlyReport {
            year: chosen.year(),
            month: chosen.month()
        }]
    );
    Ok(())
}

#[test]
fn monthly_report_offers_the_month_that_just_ended_and_keeps_the_choice()
-> Result<(), Box<dyn Error>> {
    let mut model = ready_model();
    model.month_index = 1;
    let chosen = model.months[1];
    let next_month = Zoned::now()
        .date()
        .first_of_month()
        .checked_add(1.month())?;
    assert!(model.roll_months(next_month), "A new month rolls the list");
    let ended = Zoned::now().date().first_of_month();
    assert_eq!(model.months.first(), Some(&ended));
    assert_eq!(model.months.len(), 24);
    let mut commands = Vec::new();
    model.request(Action::MonthlyReport);
    model.confirm(true, |command| commands.push(command));
    assert_eq!(
        commands,
        vec![Command::MonthlyReport {
            year: chosen.year(),
            month: chosen.month()
        }],
        "The month the operator chose stays chosen when the list rolls"
    );
    Ok(())
}

#[test]
fn resolving_a_report_needs_the_running_worker() -> Result<(), Box<dyn Error>> {
    let window = window()?;
    let mut model = ready_model();
    model.running = true;
    model.blocked = true;
    model.report_unresolved = true;
    model.request(Action::ResolveReport);
    model.stopped();
    model.sync(&window);
    assert!(
        !window.get_report_unresolved(),
        "A stopped worker cannot resolve a report"
    );
    assert_eq!(
        model.connection_text(),
        "Zablokowane: wymaga rozstrzygnięcia"
    );
    let mut commands = Vec::new();
    model.confirm(true, |command| commands.push(command));
    model.request(Action::ResolveReport);
    assert!(
        model.pending.is_none(),
        "Nothing would carry the resolution"
    );
    assert!(commands.is_empty(), "No command reaches a stopped worker");
    model.blocked = false;
    assert_eq!(model.connection_text(), "Niepołączono");
    Ok(())
}

#[test]
fn stopping_worker_accepts_no_confirmations() -> Result<(), Box<dyn Error>> {
    let window = window()?;
    let mut model = ready_model();
    model.running = true;
    model.report_unresolved = true;
    model.set_documents(vec![document()], 1, false);
    model.selected = Some(0);
    let mut commands = Vec::new();
    model.request(Action::DailyReport);
    model.stopping = true;
    model.confirm(true, |command| commands.push(command));
    for action in [
        Action::QueueReceipt,
        Action::DailyReport,
        Action::MonthlyReport,
        Action::PeriodicReport,
        Action::ResolveReport,
    ] {
        model.request(action);
        assert!(model.pending.is_none(), "{action:?} offered while stopping");
    }
    assert!(
        commands.is_empty(),
        "A stopping worker discards later commands"
    );
    model.sync(&window);
    assert!(
        !window.get_queue_enabled()
            && !window.get_reports_enabled()
            && !window.get_report_unresolved(),
        "Stopping disables every confirmed command"
    );
    Ok(())
}

#[test]
fn confirmation_does_not_submit_after_blocking_or_cancellation() {
    let mut model = Model::new(Settings::default());
    model.connected = true;
    model.set_documents(vec![document()], 1, false);
    model.selected = Some(0);
    let mut commands = Vec::new();
    model.request(Action::QueueReceipt);
    model.confirm(false, |command| commands.push(command));
    model.request(Action::QueueReceipt);
    model.blocked = true;
    model.confirm(true, |command| commands.push(command));
    assert!(
        commands.is_empty(),
        "Cancellation and a new fiscal block must prevent queuing"
    );
}

#[test]
fn typed_report_dates_must_be_valid_and_not_in_the_future() -> Result<(), Box<dyn Error>> {
    let mut model = ready_model();
    model.to = date_label(Zoned::now().date().checked_add(1.day())?);
    model.request(Action::PeriodicReport);
    assert_eq!(
        model.message,
        "Data końcowa nie może być późniejsza niż dzisiejsza."
    );
    assert!(
        model.pending.is_none(),
        "A future date must not become a report boundary"
    );
    // A two-digit year would otherwise parse as year 0025, not 2025.
    for typed in [
        "31.02.2025",
        "2025-10-20",
        "20.10",
        "",
        "20.10.25",
        "1.1.25",
    ] {
        model.from = typed.into();
        model.request(Action::PeriodicReport);
        assert_eq!(
            model.message, "Data początkowa musi mieć postać dd.mm.rrrr.",
            "{typed:?} is not a dd.mm.rrrr date"
        );
        assert!(model.pending.is_none(), "{typed:?} cannot be confirmed");
    }
    Ok(())
}

#[test]
fn sorting_and_refresh_keep_the_selected_document() {
    let mut model = Model::new(Settings::default());
    model.connected = true;
    let selected = document();
    let mut other = document();
    other.order_id = Uuid::from_u128(1);
    other.order_number = 7;
    model.set_documents(vec![selected.clone(), other.clone()], 1, false);
    model.selected = Some(0);
    model.sort_documents(0, false);
    assert_eq!(model.documents(), [other.clone(), selected.clone()]);
    assert_eq!(model.selected, Some(1));
    // The worker refreshes in server order; the operator's order stays.
    model.set_documents(vec![selected.clone(), other], 1, false);
    assert_eq!(model.selected, Some(1));
    model.request(Action::QueueReceipt);
    let mut commands = Vec::new();
    model.confirm(true, |command| commands.push(command));
    assert_eq!(
        commands,
        vec![Command::Queue {
            document: selected,
            buyer_nip: None
        }]
    );
}

#[test]
fn running_worker_settings_ignore_window_edits() -> Result<(), Box<dyn Error>> {
    let window = window()?;
    let mut model = Model::new(Settings::default());
    model.running = true;
    model.sync(&window);
    window.set_port("/dev/ttyUSB9".into());
    window.set_api_url("https://other.example".into());
    model.capture(&window);
    assert_eq!(
        model.settings,
        Settings::default(),
        "Settings change only while the worker is stopped"
    );
    Ok(())
}

fn click(window: &AppWindow, x: f32, y: f32) {
    let position = LogicalPosition::new(x, y);
    let button = PointerEventButton::Left;
    window
        .window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    window
        .window()
        .dispatch_event(WindowEvent::PointerPressed { position, button });
    window
        .window()
        .dispatch_event(WindowEvent::PointerReleased { position, button });
}

#[derive(Debug, PartialEq, Eq)]
enum Clicked {
    Row(i32),
    Sort(i32, bool),
}

#[test]
fn clicked_row_is_the_queued_document_after_sorting_and_refresh() -> Result<(), Box<dyn Error>> {
    let window = window()?;
    window.window().set_size(LogicalSize::new(1180.0, 820.0));
    let mut model = Model::new(Settings::default());
    model.connected = true;
    model.tab = Tab::Documents;
    let first = document();
    let mut second = document();
    second.order_id = Uuid::from_u128(1);
    second.order_number = 7;
    model.set_documents(vec![first.clone(), second.clone()], 1, false);
    model.initialize(&window);
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let rows = Rc::clone(&clicks);
    window.on_selected_document(move |row| rows.borrow_mut().push(Clicked::Row(row)));
    let sorts = Rc::clone(&clicks);
    window.on_sort_documents(move |column, descending| {
        sorts.borrow_mut().push(Clicked::Sort(column, descending));
    });

    // Probe down the "Zlecenie" column for its header and both rows, so layout tweaks don't
    // silently move the clicks off the table.
    let x = 300.0;
    let (mut header, mut second_row) = (None, None);
    for step in 0..205_u16 {
        let y = f32::from(step) * 4.0;
        click(&window, x, y);
        match clicks.borrow_mut().pop() {
            Some(Clicked::Sort(..)) => header = header.or(Some(y)),
            Some(Clicked::Row(1)) => {
                second_row = Some(y);
                break;
            },
            Some(Clicked::Row(_)) | None => {},
        }
    }
    let header = header.ok_or("no column header under the documents table's first column")?;
    let second_row = second_row.ok_or("no second document row")?;
    // Probing toggled the column's indicator; Rust's (unsorted) state restores it.
    model.sync(&window);
    clicks.borrow_mut().clear();

    click(&window, x, second_row);
    assert_eq!(
        *clicks.borrow(),
        [Clicked::Row(1)],
        "A row click reports its visible index"
    );
    model.selected = Some(1);
    click(&window, x, header);
    assert_eq!(
        clicks.borrow().last(),
        Some(&Clicked::Sort(0, false)),
        "The first header click sorts by order number, ascending"
    );
    model.sort_documents(0, false);
    model.sync(&window);
    assert_eq!(
        window.get_selected(),
        0,
        "Order 7 now leads and stays highlighted"
    );
    // The worker refreshes in server order; the operator's order and selection stay.
    model.set_documents(vec![first, second.clone()], 1, false);
    model.sync(&window);
    assert_eq!(window.get_selected(), 0);

    model.request(Action::QueueReceipt);
    let mut commands = Vec::new();
    model.confirm(true, |command| commands.push(command));
    assert_eq!(
        commands,
        vec![Command::Queue {
            document: second,
            buyer_nip: None
        }],
        "The queued receipt is the document the operator clicked"
    );
    Ok(())
}

fn printed(receipt_number: Option<&str>, items: &[&str]) -> HistoryEntry {
    HistoryEntry {
        job_id: Uuid::nil(),
        printed_at: Timestamp::UNIX_EPOCH,
        receipt_number: receipt_number.map(str::to_owned),
        total: Grosze::ZERO,
        payments: vec![],
        items: items.iter().copied().map(str::to_owned).collect(),
    }
}

#[test]
fn receipt_numbers_sort_by_value() {
    let mut model = Model::new(Settings::default());
    model.set_history(
        [Some("10"), Some("staff-7"), Some("9"), None, Some("2")]
            .map(|number| printed(number, &[]))
            .into(),
    );
    model.sort_history(2, false);
    let numbers: Vec<_> = model
        .history
        .items()
        .iter()
        .map(|entry| entry.receipt_number.as_deref())
        .collect();
    assert_eq!(
        numbers,
        [None, Some("2"), Some("9"), Some("10"), Some("staff-7")],
        "The printer's counter orders numerically; staff text follows"
    );
}

#[test]
fn names_sort_in_polish_alphabetical_order() {
    let mut model = Model::new(Settings::default());
    let customers = ["Żak", "Zenon", "adam", "Łukasz", "Lz", "Bartek"];
    model.set_documents(
        (0..)
            .zip(customers)
            .map(|(id, name)| Document {
                order_id: Uuid::from_u128(id),
                customer_name: Some(name.into()),
                ..document()
            })
            .collect(),
        1,
        false,
    );
    model.sort_documents(2, false);
    let sorted: Vec<_> = model
        .documents()
        .iter()
        .map(|document| document.customer_name.as_deref())
        .collect();
    assert_eq!(
        sorted,
        ["adam", "Bartek", "Lz", "Łukasz", "Zenon", "Żak"].map(Some)
    );
    model.set_history(
        [["Tarcza"], ["Śruba"], ["olej"], ["Sprężyna"]]
            .map(|items| printed(None, &items))
            .into(),
    );
    model.sort_history(0, false);
    let sorted: Vec<_> = model
        .history
        .items()
        .iter()
        .map(|entry| entry.items.as_slice())
        .collect();
    assert_eq!(sorted, [["olej"], ["Sprężyna"], ["Śruba"], ["Tarcza"]]);
}
