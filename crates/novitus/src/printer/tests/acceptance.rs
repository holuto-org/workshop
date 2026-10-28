use super::*;

#[test]
fn demo_mode_enables_unqualified_capabilities_through_the_real_driver() {
    for full in [false, true] {
        let sim = Simulator::start();
        supported(&sim);
        sim.set_fiscal_mode(0);
        sim.set_status(0x74, 0x65, "@s0;0;0;00000000;");
        sim.set_header("Inny podatnik");
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        assert!(expected.ready(), "{}", expected.description());
        let advances = [(0, if full { 1000 } else { 500 }), (1, 2000), (4, 3000)]
            .map(|(slot, amount)| Advance {
                vat: VatSlot::new(slot).unwrap(),
                amount: Grosze::new(amount).unwrap(),
            })
            .to_vec();
        let mut log = Log::default();
        let printed = driver
            .print_receipt(&receipt(advances), &expected, &mut log)
            .unwrap();
        assert_eq!(printed.receipt_number.as_deref(), Some("1"));
        assert_eq!(sim.receipts(), 1);
        let mut frames = vec![
            "0;1$hWarsztat\r",
            "100;1$z1234567890\r",
            "1$lNaprawa\r1\rA/10/10/",
            if full {
                "1;@zNaprawa\rA\r10/0/"
            } else {
                "1;@zNaprawa\rA\r5/5/"
            },
            "2$lOlej\r1\rB/20/20/",
            "1;@zOlej\rB\r20/0/",
            "3$lPrzeglad\r1\rE/30/30/",
            "1;@zPrzeglad\rE\r30/0/",
        ];
        if !full {
            frames.push("1;0$b5/\r");
        }
        frames.extend([
            if full {
                "0;3;0;0;0;0;0;0;0;0;1;0$y\r\r\r0/0/0/0/0/"
            } else {
                "0;3;0;0;0;0;0;0;0;0;1;0$y\r\r\r5/0/0/5/0/"
            },
            "12$z",
            "28;0$z",
        ]);
        assert_eq!(fiscal_bodies(&sim), frames);
        let mut commands = vec!["$h", "$z", "$l", "@z", "$l", "@z", "$l", "@z"];
        if !full {
            commands.push("$b");
        }
        commands.extend(["$y", "$z", "$z"]);
        assert_eq!(log.commands, commands);
    }
}

#[test]
fn advance_spans_same_vat_items_and_footer_keeps_exact_cp1250() {
    let sim = Simulator::start();
    supported(&sim);
    sim.set_header("Inny podatnik");
    let driver = printer(&sim);
    let expected = driver.probe().unwrap();
    let caps = &expected.capabilities;
    let lines = [("Pierwszy", 1000), ("Drugi", 2000), ("Trzeci", 3000)]
        .map(|(name, value)| {
            Line::new(
                name,
                Quantity::parse("1").unwrap(),
                Grosze::new(value).unwrap(),
                VatSlot::new(0).unwrap(),
                caps,
            )
            .unwrap()
        })
        .to_vec();
    let job = Receipt::new(
        lines,
        vec![Advance {
            vat: VatSlot::new(0).unwrap(),
            amount: Grosze::new(1500).unwrap(),
        }],
        vec![
            Payment {
                form: PaymentForm::Cash,
                amount: Grosze::new(4500).unwrap(),
            },
            Payment {
                form: PaymentForm::Mobile,
                amount: Grosze::ZERO,
            },
        ],
        None,
        "Łódź",
        caps,
    )
    .unwrap();
    let mut log = Log::default();
    let printed = driver.print_receipt(&job, &expected, &mut log).unwrap();
    assert_eq!(printed.receipt_number.as_deref(), Some("1"));
    assert_eq!(
        fiscal_bodies(&sim),
        [
            "0;1$hŁódź\r",
            "1$lPierwszy\r1\rA/10/10/",
            "1;@zPierwszy\rA\r10/0/",
            "2$lDrugi\r1\rA/20/20/",
            "1;@zDrugi\rA\r5/15/",
            "3$lTrzeci\r1\rA/30/30/",
            "1;0$b45/\r",
            "0;3;0;0;0;0;0;0;0;0;1;0$y\r\r\r45/0/0/45/0/",
            "12$z",
            "28;0$z",
        ]
    );
    assert_eq!(
        log.commands,
        ["$h", "$l", "@z", "$l", "@z", "$l", "$b", "$y", "$z", "$z"]
    );
    assert!(
        sim.frames()
            .iter()
            .any(|raw| raw.windows(4).any(|part| part == [0xA3, 0xF3, b'd', 0x9F]))
    );
    assert_eq!(sim.receipts(), 1);
}

#[test]
fn demo_mode_keeps_every_other_readiness_gate() {
    for problem in 0..11 {
        let sim = Simulator::start();
        supported(&sim);
        sim.set_fiscal_mode(0);
        sim.set_status(0x74, 0x65, "@s0;0;0;00000000;");
        match problem {
            0 => sim.set_fiscal_mode(2),
            1 => sim.set_endpoint("PC2"),
            2 => sim.set_dialect("0"),
            3 => sim.set_config_option(92, "0"),
            4 => sim.set_header(""),
            5 => sim.set_status(0x76, 0x65, "@s0;0;0;00000000;"),
            6 => sim.set_status(0x74, 0x67, "@s0;0;0;00000000;"),
            7 => sim.set_identity("NOVITUS HD II ONLINE", "99.00"),
            8 => sim.set_config_option(91, "0"),
            9 => sim.set_vat(["99.99"; 7]),
            _ => sim.set_status(0x74, 0x65, "@s0;0;0;80000000;"),
        }
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        assert!(!expected.ready(), "case {problem}");
        let mut log = Log::default();
        assert!(matches!(
            driver.print_receipt(&receipt(vec![]), &expected, &mut log),
            Err(Error::NotReady(message)) if message.to_string() == expected.description()
        ));
        assert_eq!(log.commands, Vec::<&str>::new());
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
        assert_eq!((sim.receipts(), sim.reports()), (0, 0));
    }
}
