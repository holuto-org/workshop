use super::*;

#[test]
fn demo_mode_enables_unqualified_capabilities_through_the_real_driver() {
    for advance in [2346, 12346] {
        let sim = Simulator::start();
        sim.set_status(0, (1_u128 << 102) | (1_u128 << 98) | 4 | 8, 32);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        assert!(expected.ready(), "{}", expected.description());
        let mut log = Log::default();
        let result = driver.print_receipt(&receipt(advance, true), &expected, &mut log);
        if advance == 12346 {
            // Acceptance permits the attempt, not a fabricated zero-close success.
            assert!(matches!(
                result,
                Err(Error::Uncertain(error))
                    if matches!(*error, Error::Device { code: 1008, .. })
            ));
            assert_eq!(sim.receipts(), 0);
        } else {
            assert_eq!(result.unwrap().receipt_number.as_deref(), Some("1"));
            assert_eq!(sim.receipts(), 1);
        }
        let mut frames = vec![
            "LOR\x1dC0",
            "LNP\x1dOA1234567890",
            "LIT\x1dGAŁożysko\x1eBA\x1eC1,23456\x1eD100,00\x1eE123,46",
        ];
        if advance == 12346 {
            frames.extend([
                "LDS\x1dIAA1\x1fB123,46\x1fDZaliczka\x1eCA\x1eD",
                "LCL\x1dRA0,00",
            ]);
            assert_eq!(log.commands, ["LOR", "LNP", "LIT", "LDS", "LCL"]);
            assert!(sim.transaction_open());
        } else {
            frames.extend([
                "LDS\x1dIAA1\x1fB23,46\x1fDZaliczka\x1eCA\x1eD",
                "LBP\x1dKA1\x1eBKARTA\x1eC100,00\x1eX2",
                "LCL\x1dRA100,00",
                "LAL\x1dQAAInna firma",
            ]);
            assert_eq!(
                log.commands,
                ["LOR", "LNP", "LIT", "LDS", "LBP", "LCL", "LAL"]
            );
            assert!(!sim.transaction_open());
        }
        assert_eq!(fiscal(&sim), frames);
    }
}

#[test]
fn demo_mode_keeps_readonly_mechanism_identity_and_codepage_gates() {
    for problem in 0..6 {
        let sim = Simulator::start();
        let training = (1_u128 << 102) | (1_u128 << 98) | 4 | 8;
        sim.set_status(0, training, 32);
        match problem {
            0 => sim.set_status(0, training & !4, 32),
            1 => sim.set_status(0, training | (1_u128 << 68), 32),
            2 => sim.set_status(0, training, 34),
            3 => sim.set_code_page("CP852"),
            4 => sim.set_header(""),
            _ => sim.set_identity("ELZAB UNKNOWN ONLINE", "MD06.02"),
        }
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        assert!(!expected.ready(), "case {problem}");
        let mut log = Log::default();
        assert!(matches!(
            driver.print_receipt(&receipt(0, false), &expected, &mut log),
            Err(Error::NotReady(_))
        ));
        assert_eq!(log.commands, Vec::<&str>::new());
        assert_eq!(fiscal(&sim), Vec::<String>::new());
    }
}

#[test]
fn acceptance_does_not_promote_daily_report_ack_to_completion() {
    let sim = Simulator::start();
    sim.set_status(0, (1_u128 << 102) | (1_u128 << 98) | 4 | 8, 32);
    let mut log = Log::default();
    assert!(matches!(
        printer(&sim).print_report(Report::Daily, "ABC1234567890", &mut log),
        Err(Error::Uncertain(_))
    ));
    assert_eq!(log.commands, ["RRP"]);
    assert_eq!(fiscal(&sim), ["RRP\x1dI1\x1dJ1"]);
}

#[test]
fn unqualified_footer_faults_keep_the_printed_effect_and_do_not_replay() {
    for fault in [
        Fault::LoseReply,
        Fault::CorruptReply,
        Fault::Disconnect,
        Fault::Device(450),
        Fault::Transport("SL".to_owned()),
    ] {
        let sim = Simulator::start();
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        sim.fault("LAL", fault);
        let mut log = Log::default();
        assert!(matches!(
            driver.print_receipt(&receipt(2346, true), &expected, &mut log),
            Err(Error::Uncertain(_))
        ));
        assert_eq!(
            log.commands,
            ["LOR", "LNP", "LIT", "LDS", "LBP", "LCL", "LAL"]
        );
        assert_eq!(
            fiscal(&sim),
            [
                "LOR\x1dC0",
                "LNP\x1dOA1234567890",
                "LIT\x1dGAŁożysko\x1eBA\x1eC1,23456\x1eD100,00\x1eE123,46",
                "LDS\x1dIAA1\x1fB23,46\x1fDZaliczka\x1eCA\x1eD",
                "LBP\x1dKA1\x1eBKARTA\x1eC100,00\x1eX2",
                "LCL\x1dRA100,00",
                "LAL\x1dQAAInna firma",
            ]
        );
        assert_eq!((sim.receipts(), sim.transaction_open()), (1, false));
    }
}
