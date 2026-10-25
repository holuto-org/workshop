use super::*;
use agent::documents::ReceiptKind;
use fiscal::device::Problem;
use posnet::capabilities::capabilities;
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
        due_at: Timestamp::UNIX_EPOCH,
        latest_at: Timestamp::UNIX_EPOCH,
        is_overdue: false,
    }
}
fn ready() -> Model {
    let mut model = Model::new(Settings::default(), today());
    model.connected = true;
    model.device = Some(Device {
        vendor: Vendor::Posnet,
        model: String::new(),
        firmware: String::new(),
        unique_number: "TST0000000001".to_owned(),
        problems: Vec::new(),
        vat: Vec::new(),
        capabilities: capabilities(),
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
fn daily_report_confirmation_rechecks_new_readiness_problems() {
    let mut model = ready();
    let device = model.device.as_mut().unwrap();
    device.problems.push(Problem::DailyReportOverdue);
    model.request(Action::DailyReport);
    assert!(model.pending.is_some());
    model
        .device
        .as_mut()
        .unwrap()
        .problems
        .push("Brak papieru".into());
    let mut commands = Vec::new();
    model.confirm(true, |command| commands.push(command));
    assert_eq!(commands, []);
}

#[test]
fn a_read_only_printer_offers_monthly_and_periodic_reports_but_not_the_daily_report() {
    let mut model = ready();
    model.from = "01.10.2025".into();
    model.to = "24.10.2025".into();
    model
        .device
        .as_mut()
        .unwrap()
        .problems
        .push(Problem::ReadOnly);
    let mut commands = Vec::new();
    for action in [
        Action::DailyReport,
        Action::MonthlyReport,
        Action::PeriodicReport,
    ] {
        model.request(action);
        model.confirm(true, |command| commands.push(command));
        model.command_completed();
    }
    assert!(
        matches!(
            commands.as_slice(),
            [
                Command::MonthlyReport { .. },
                Command::PeriodicReport { .. }
            ]
        ),
        "{commands:?}"
    );
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

#[test]
fn stopped_status_retains_identity_and_fiscal_blocks_but_no_private_model_data() {
    let mut model = ready();
    model.running = true;
    model.vendor = Some(Vendor::Posnet);
    model.unique_number = "SERIAL42".into();
    model.printer_id = Some("printer-42".into());
    model.blocked = true;
    model.report_unresolved = true;
    model.orphaned_receipt = Some("Paragon 17".into());
    model.settings.api_url = "https://private-api.example.test".into();
    model.settings.token = "private-token-canary".into();
    model.nip = "1234567890".into();
    let updated_at = Timestamp::UNIX_EPOCH;
    model.stopped();
    assert_eq!(
        model.status(updated_at),
        Snapshot {
            processing: false,
            connected: false,
            unique_number: Some("SERIAL42".into()),
            vendor: Some(Vendor::Posnet),
            printer_id: Some("printer-42".into()),
            blocked: true,
            report_unresolved: true,
            orphaned_receipt: true,
            updated_at,
        },
        "stopping disconnects without erasing unresolved fiscal evidence"
    );
}

#[test]
fn each_public_status_change_invalidates_the_previous_publication() {
    let changes: [fn(&mut Model); 8] = [
        |model| model.running = true,
        |model| model.connected = true,
        |model| model.unique_number = "SERIAL42".into(),
        |model| model.vendor = Some(Vendor::Novitus),
        |model| model.printer_id = Some("printer-42".into()),
        |model| model.blocked = true,
        |model| model.report_unresolved = true,
        |model| model.orphaned_receipt = Some("Paragon 17".into()),
    ];
    for change in changes {
        let mut model = Model::new(Settings::default(), today());
        let published = model.status(Timestamp::UNIX_EPOCH);
        assert!(model.status_matches(&published));
        change(&mut model);
        assert!(
            !model.status_matches(&published),
            "changed state needs a new publication"
        );
    }
}
