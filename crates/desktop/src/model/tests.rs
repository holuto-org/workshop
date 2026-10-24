use super::*;
use agent::documents::ReceiptKind;
use jiff::Timestamp;
use uuid::Uuid;

fn today() -> Date {
    Date::constant(2025, 10, 24)
}
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
fn ready() -> Model {
    let mut model = Model::new(Settings::default(), today());
    model.connected = true;
    model.device = Some(Status {
        name: String::new(),
        serial: String::new(),
        version: String::new(),
        problems: Vec::new(),
        vat: Vec::new(),
    });
    model.set_documents(vec![document()], 1, false);
    model.selected = Some(0);
    model
}

#[test]
fn confirmed_actions_cannot_be_submitted_again_until_worker_completion() {
    for action in [
        Action::QueueReceipt,
        Action::DailyReport,
        Action::MonthlyReport,
        Action::PeriodicReport,
    ] {
        let mut model = ready();
        let mut commands = Vec::new();
        model.request(action);
        model.confirm(true, |command| commands.push(command));
        model.request(action);
        model.confirm(true, |command| commands.push(command));
        assert_eq!(
            commands.len(),
            1,
            "second confirmation must not queue another fiscal operation"
        );
        model.command_completed();
        model.request(action);
        model.confirm(true, |command| commands.push(command));
        assert_eq!(commands.len(), 2, "worker completion unlocks the action");
    }
}

#[test]
fn periodic_dates_require_padded_day_month_and_year_at_least_2000() {
    for (from, to, message) in [
        (
            "31.12.1999",
            "01.01.2000",
            "Data początkowa nie może być wcześniejsza niż 01.01.2000.",
        ),
        (
            "1.1.2025",
            "24.10.2025",
            "Data początkowa musi mieć postać dd.mm.rrrr.",
        ),
        (
            "01.01.2025",
            "1.1.2025",
            "Data końcowa musi mieć postać dd.mm.rrrr.",
        ),
        (
            "25.10.2025",
            "25.10.2025",
            "Data początkowa nie może być późniejsza niż dzisiejsza.",
        ),
        (
            "24.10.2025",
            "23.10.2025",
            "Data początkowa nie może być późniejsza niż końcowa.",
        ),
        (
            "31.02.2025",
            "24.10.2025",
            "Data początkowa musi mieć postać dd.mm.rrrr.",
        ),
    ] {
        let error = report_range(from, to, today())
            .err()
            .map(|error| error.to_string());
        assert_eq!(error.as_deref(), Some(message), "range {from} – {to}");
    }
    assert_eq!(
        report_range("01.01.2000", "24.10.2025", today()).ok(),
        Some((Date::constant(2000, 1, 1), today())),
        "both inclusive boundaries are accepted"
    );
}

#[test]
fn cancelled_or_newly_blocked_confirmation_sends_nothing() {
    for accepted in [false, true] {
        let mut model = ready();
        model.request(Action::QueueReceipt);
        model.blocked = accepted;
        let mut commands = Vec::new();
        model.confirm(accepted, |command| commands.push(command));
        assert!(
            commands.is_empty(),
            "cancelled or blocked receipt must not be sent"
        );
    }
}

#[test]
fn month_rollover_preserves_the_operator_choice() {
    let mut model = Model::new(Settings::default(), Date::constant(2025, 10, 31));
    model.month_index = 1;
    assert!(
        model.roll_months(Date::constant(2025, 11, 1)),
        "month list rolls at midnight"
    );
    assert_eq!(
        model.months.first(),
        Some(&Date::constant(2025, 10, 1)),
        "just-ended month is available"
    );
    assert_eq!(
        model.months.get(model.month_index),
        Some(&Date::constant(2025, 8, 1)),
        "selected month survives rollover"
    );
}

#[test]
fn sorted_refresh_retains_document_identity_not_row_index() {
    let mut model = ready();
    let mut other = document();
    other.order_id = Uuid::from_u128(1);
    other.order_number = 1;
    model.set_documents(vec![document(), other.clone()], 1, true);
    model.sort_documents(0, false);
    assert_eq!(
        model.selected,
        Some(1),
        "sort follows the selected document"
    );
    model.set_documents(vec![other, document()], 2, false);
    model.request(Action::QueueReceipt);
    let mut commands = Vec::new();
    model.confirm(true, |command| commands.push(command));
    assert!(
        matches!(
            commands.as_slice(),
            [Command::Queue { document, .. }] if document.order_number == 42
        ),
        "refresh must not queue the old row index"
    );
}
