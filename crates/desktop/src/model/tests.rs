#![expect(
    clippy::panic_in_result_fn,
    reason = "Tests assert behavior while propagating fallible fixture and UI setup"
)]

use std::error::Error;

use super::*;
use agent::documents::ReceiptKind;
use uuid::Uuid;

fn document() -> Document {
    Document {
        kind: ReceiptKind::Sale,
        order_id: Uuid::nil(),
        order_number: 42,
        allocation_id: None,
        amount: Decimal::new(25000, 2),
        customer_name: Some("Jan Kowalski".into()),
        occurred_at: Timestamp::UNIX_EPOCH,
    }
}

#[test]
fn controls_follow_connection_selection_readiness_and_blocking() -> Result<(), Box<dyn Error>> {
    i_slint_backend_testing::init_no_event_loop();
    let window = AppWindow::new()?;
    let mut model = Model::new(Settings::default());
    model.documents = vec![document()];
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
    model.documents = vec![document()];
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
fn report_commands_and_date_order() -> Result<(), Box<dyn Error>> {
    let mut model = Model::new(Settings::default());
    model.connected = true;
    model.device = Some(Status {
        name: String::new(),
        version: String::new(),
        serial: String::new(),
        problems: vec![],
        vat: vec![],
    });
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
                year: month.0,
                month: month.1
            }
        ]
    );
    model.from = Date::new(2026, 10, 20)?;
    model.to = Date::new(2026, 10, 19)?;
    model.request(Action::PeriodicReport);
    assert_eq!(
        model.message,
        "Data początkowa nie może być późniejsza niż końcowa."
    );
    assert!(
        model.pending.is_none(),
        "Reversed dates cannot be confirmed"
    );
    model.to = Date::new(2026, 10, 21)?;
    model.summary = true;
    model.request(Action::PeriodicReport);
    model.confirm(true, |command| commands.push(command));
    assert_eq!(
        commands.last(),
        Some(&Command::PeriodicReport {
            from: model.from,
            to: model.to,
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
    model.documents = vec![selected.clone(), other.clone()];
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
    model.documents = vec![document()];
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
fn future_date_is_refused_without_changing_selected_range() -> Result<(), Box<dyn Error>> {
    let mut model = Model::new(Settings::default());
    let old = model.to;
    let tomorrow = old.checked_add(Span::new().days(1))?;
    model.change_date(false, &ui_date(tomorrow));
    assert_eq!(
        model.to, old,
        "A future date must not become a report boundary"
    );
    assert_eq!(
        model.message,
        "Data nie może być późniejsza niż dzisiejsza."
    );
    Ok(())
}
