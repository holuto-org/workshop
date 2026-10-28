use std::slice;

use super::*;
use fiscal::device::Problem;
use posnet::capabilities::capabilities;

fn today() -> Date {
    Date::constant(2025, 10, 24)
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
    model
}

#[test]
fn confirmed_actions_cannot_be_submitted_again_until_worker_completion() {
    for (action, expected) in [
        (Action::DailyReport, Command::DailyReport),
        (
            Action::MonthlyReport,
            Command::MonthlyReport {
                year: 2025,
                month: 9,
            },
        ),
        (
            Action::PeriodicReport,
            Command::PeriodicReport {
                from: Date::constant(2025, 10, 17),
                to: today(),
                summary: false,
            },
        ),
        (Action::ResolveReport, Command::ResolveReport),
        (
            Action::AcknowledgeOrphanedReceipt,
            Command::AcknowledgeOrphanedReceipt,
        ),
    ] {
        let mut model = ready();
        model.report_unresolved = true;
        model.orphaned_receipt = Some("Zlecenie 42 · paragon 41".into());
        let mut commands = Vec::new();
        model.request(action);
        commands.extend(model.confirm(true));
        model.request(action);
        commands.extend(model.confirm(true));
        assert_eq!(
            commands.as_slice(),
            slice::from_ref(&expected),
            "second confirmation is consumed once"
        );
        model.command_completed();
        model.request(action);
        commands.extend(model.confirm(true));
        assert_eq!(
            commands.iter().collect::<Vec<_>>(),
            [&expected, &expected],
            "completion unlocks the action"
        );
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
    commands.extend(model.confirm(true));
    assert_eq!(commands, []);
}

#[test]
fn accepting_a_confirmation_the_stopped_printer_can_no_longer_run_says_nothing_was_sent() {
    let mut model = ready();
    model.request(Action::DailyReport);
    assert!(model.pending.is_some());
    model.stopped();
    let mut commands = Vec::new();
    commands.extend(model.confirm(true));
    assert_eq!(commands, []);
    assert_eq!(
        model.message, "Nie wysłano: stan drukarki zmienił się przed potwierdzeniem.",
        "the operator must not take the closed dialog for a printed report"
    );
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
        .push(Problem::ReadOnly {
            periodic_reports: true,
        });
    let mut commands = Vec::new();
    for action in [
        Action::DailyReport,
        Action::MonthlyReport,
        Action::PeriodicReport,
    ] {
        model.request(action);
        commands.extend(model.confirm(true));
        model.command_completed();
    }
    assert_eq!(
        commands,
        [
            Command::MonthlyReport {
                year: 2025,
                month: 9
            },
            Command::PeriodicReport {
                from: Date::constant(2025, 10, 1),
                to: today(),
                summary: false,
            },
        ]
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
        (
            " 1.01.2025",
            "24.10.2025",
            "Data początkowa musi mieć postać dd.mm.rrrr.",
        ),
        (
            "01. 1.2025",
            "24.10.2025",
            "Data początkowa musi mieć postać dd.mm.rrrr.",
        ),
        (
            "01.01.2025",
            "25.10.2025",
            "Data końcowa nie może być późniejsza niż dzisiejsza.",
        ),
        (
            "29.02.2025",
            "24.10.2025",
            "Data początkowa musi mieć postać dd.mm.rrrr.",
        ),
    ] {
        let error = report_range(from, to, today()).err();
        assert_eq!(error.as_deref(), Some(message), "range {from} – {to}");
    }
    assert_eq!(
        report_range("01.01.2000", "24.10.2025", today()).ok(),
        Some((Date::constant(2000, 1, 1), today())),
        "both inclusive boundaries are accepted"
    );
    assert_eq!(
        report_range("29.02.2024", "29.02.2024", today()).unwrap(),
        (Date::constant(2024, 2, 29), Date::constant(2024, 2, 29)),
        "a valid leap day and equal endpoints are accepted"
    );
}

#[test]
fn cancelled_or_newly_blocked_confirmation_sends_nothing() {
    for accepted in [false, true] {
        let mut model = ready();
        model.request(Action::DailyReport);
        model.blocked = accepted;
        let mut commands = Vec::new();
        commands.extend(model.confirm(accepted));
        assert!(
            commands.is_empty(),
            "a cancelled or newly blocked report must not be sent"
        );
    }
}

#[test]
fn a_stop_requested_while_a_confirmation_is_open_sends_nothing() {
    for action in [
        Action::DailyReport,
        Action::MonthlyReport,
        Action::PeriodicReport,
        Action::ResolveReport,
        Action::AcknowledgeOrphanedReceipt,
    ] {
        let mut model = ready();
        model.report_unresolved = true;
        model.orphaned_receipt = Some("Zlecenie 42 · paragon 41".into());
        model.request(action);
        assert!(model.pending.is_some(), "{action:?} opens a confirmation");
        model.stopping = true;
        let mut commands = Vec::new();
        commands.extend(model.confirm(true));
        assert_eq!(commands, [], "{action:?} must not reach a stopping worker");
        assert_eq!(
            model.message, "Nie wysłano: stan drukarki zmienił się przed potwierdzeniem.",
            "{action:?} must say nothing was sent"
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

#[test]
fn capture_rejects_assistive_connection_edits_while_running_saving_or_pairing() {
    i_slint_backend_testing::init_no_event_loop();
    for locked in 0..3 {
        let settings = Settings {
            api_url: "https://paired.example.test".into(),
            token: "synthetic-paired-token".into(),
            port: "saved-port".into(),
            ..Settings::default()
        };
        let mut model = Model::new(settings.clone(), today());
        let window = AppWindow::new().unwrap();
        model.initialize(&window, today());
        model.running = locked == 0;
        model.saving = locked == 1;
        model.pairing = locked == 2;
        window.set_port("unsaved-port".into());
        window.set_baud_index(0);
        window.set_pairing_code("ABCD1234".into());
        model.capture(&window, today());
        model.sync(&window);
        assert_eq!(model.settings, settings);
        assert_eq!(window.get_port(), "saved-port");
        assert_eq!(window.get_pairing_code(), "");
    }
}

#[test]
fn window_initialization_lists_every_polish_month_and_tab_capture_round_trips() {
    use slint::Model as _;
    i_slint_backend_testing::init_no_event_loop();
    let window = AppWindow::new().unwrap();
    let mut model = Model::new(Settings::default(), Date::constant(2026, 1, 1));
    model.initialize(&window, Date::constant(2026, 1, 1));
    assert_eq!(
        window
            .get_months()
            .iter()
            .map(|month| month.to_string())
            .collect::<Vec<_>>(),
        [
            "grudzień 2025",
            "listopad 2025",
            "październik 2025",
            "wrzesień 2025",
            "sierpień 2025",
            "lipiec 2025",
            "czerwiec 2025",
            "maj 2025",
            "kwiecień 2025",
            "marzec 2025",
            "luty 2025",
            "styczeń 2025",
            "grudzień 2024",
            "listopad 2024",
            "październik 2024",
            "wrzesień 2024",
            "sierpień 2024",
            "lipiec 2024",
            "czerwiec 2024",
            "maj 2024",
            "kwiecień 2024",
            "marzec 2024",
            "luty 2024",
            "styczeń 2024",
        ]
    );
    for index in [0, 1, 2] {
        window.set_tab(index);
        model.capture(&window, Date::constant(2026, 1, 1));
        model.sync(&window);
        assert_eq!(window.get_tab(), index);
    }
}

fn stan(model: &Model) -> [String; 5] {
    [
        model.processing_text().to_owned(),
        model.holuto_text().to_owned(),
        model.printer_identity(),
        model
            .printer_problem()
            .unwrap_or_else(|| "Gotowa".to_owned()),
        model.start_hint().to_owned(),
    ]
}

#[test]
fn stan_follows_the_worker_from_stopped_through_ready_and_blocked_to_stopping() {
    let mut model = Model::new(Settings::default(), today());
    let start = "Uruchom obsługę drukarki w Ustawieniach.";
    assert_eq!(
        stan(&model),
        [
            "Zatrzymana",
            "Nie połączono",
            "—",
            "Niegotowa: brak połączenia",
            start
        ]
    );
    model.starting = true;
    assert_eq!(
        stan(&model),
        [
            "Uruchamianie…",
            "Nie połączono",
            "—",
            "Niegotowa: brak połączenia",
            ""
        ]
    );
    model.starting = false;
    model.running = true;
    assert_eq!(
        stan(&model),
        [
            "Uruchomiona",
            "Łączenie…",
            "—",
            "Niegotowa: brak połączenia",
            ""
        ]
    );
    let ready_device = ready().device;
    model.connected = true;
    model.vendor = Some(Vendor::Novitus);
    model.unique_number = "NOV0000000001".into();
    model.settings.port = "/dev/ttyUSB0".into();
    assert_eq!(
        stan(&model),
        [
            "Uruchomiona",
            "Połączono",
            "Novitus · NOV0000000001 · /dev/ttyUSB0",
            "Niegotowa: brak połączenia",
            ""
        ],
        "a connection is not ready until the printer reported its state"
    );
    model.device = ready_device;
    assert_eq!(
        stan(&model),
        [
            "Uruchomiona",
            "Połączono",
            "Novitus · NOV0000000001 · /dev/ttyUSB0",
            "Gotowa",
            ""
        ]
    );
    model
        .device
        .as_mut()
        .unwrap()
        .problems
        .push("Brak papieru".into());
    assert_eq!(stan(&model)[3], "Niegotowa: Brak papieru");
    model.blocked = true;
    assert_eq!(
        stan(&model)[3],
        "Zablokowana: wymaga rozstrzygnięcia",
        "a fiscal block outranks every readiness problem"
    );
    model.stopping = true;
    assert_eq!(stan(&model)[0], "Zatrzymywanie…");
    model.stopped();
    assert_eq!(
        stan(&model),
        [
            "Zatrzymana",
            "Nie połączono",
            "Novitus · NOV0000000001 · /dev/ttyUSB0",
            "Zablokowana: wymaga rozstrzygnięcia",
            start
        ],
        "Stop keeps the last printer identity and the fiscal block"
    );
}

#[test]
fn stan_names_every_brand_and_omits_an_unknown_port() {
    let mut model = Model::new(Settings::default(), today());
    model.unique_number = "DEM0000000001".into();
    assert_eq!(model.printer_identity(), "—", "no brand, no identity");
    for (vendor, label) in [
        (Vendor::Posnet, "Posnet"),
        (Vendor::Novitus, "Novitus"),
        (Vendor::Elzab, "Elzab"),
    ] {
        model.vendor = Some(vendor);
        assert_eq!(model.printer_identity(), format!("{label} · DEM0000000001"));
    }
    model.unique_number.clear();
    assert_eq!(
        model.printer_identity(),
        "—",
        "no unique number, no identity"
    );
}

#[test]
fn stopping_a_pairing_worker_never_reads_as_stopping_the_printer() {
    let mut model = Model::new(Settings::default(), today());
    model.pairing = true;
    model.stopping = true;
    assert_eq!(model.processing_text(), "Zatrzymana");
}
