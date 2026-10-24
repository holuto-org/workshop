#![expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture and UI setup"
)]

use std::error::Error;

use super::*;
use agent::documents::ReceiptKind;
use jiff::Timestamp;
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

#[test]
fn controls_follow_connection_selection_readiness_and_blocking() -> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_no_event_loop();
    let window = AppWindow::new()?;
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

#[test]
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
    for typed in ["31.02.2025", "2025-10-20", "20.10", ""] {
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
    i_slint_backend_testing::init_no_event_loop();
    let window = AppWindow::new()?;
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
