use super::*;

#[test]
fn demo_mode_enables_unqualified_capabilities_through_the_real_driver() {
    for advance in [2346, 12346] {
        let sim = Simulator::start();
        sim.set_status(0, (1_u128 << 102) | (1_u128 << 98) | 4 | 8, 32);
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        assert!(expected.ready(), "{}", expected.description());
        assert!(expected.capabilities.advance_deductions);
        assert!(expected.capabilities.zero_total_receipts);
        assert!(expected.capabilities.company_footer);
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
        let frames = fiscal(&sim);
        assert_eq!(
            frames.last().unwrap(),
            if advance == 12346 {
                "LCL\x1dRA0,00"
            } else {
                "LAL\x1dQAAInna firma"
            }
        );
        assert_eq!(
            frames.iter().filter(|body| body.starts_with("LDS")).count(),
            1
        );
        assert!(frames.iter().any(|body| body
            == if advance == 12346 {
                "LCL\x1dRA0,00"
            } else {
                "LCL\x1dRA100,00"
            }));
        assert_eq!(log.commands.len(), frames.len());
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
