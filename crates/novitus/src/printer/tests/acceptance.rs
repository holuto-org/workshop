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
        assert!(expected.capabilities.advance_deductions);
        assert!(expected.capabilities.zero_total_receipts);
        assert!(expected.capabilities.company_footer);
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
        let frames = fiscal_bodies(&sim);
        assert_eq!(frames.iter().filter(|body| body.contains("@z")).count(), 3);
        assert_eq!(frames.first().unwrap(), "0;1$hWarsztat\r");
        assert_eq!(frames.last().unwrap(), "28;0$z");
        assert_eq!(
            frames.iter().filter(|body| body.contains("$b")).count(),
            usize::from(!full)
        );
        assert_eq!(log.commands.len(), frames.len());
    }
}

#[test]
fn demo_mode_keeps_every_other_readiness_gate() {
    for problem in 0..8 {
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
            _ => sim.set_identity("NOVITUS HD II ONLINE", "99.00"),
        }
        let driver = printer(&sim);
        let expected = driver.probe().unwrap();
        assert!(!expected.ready(), "case {problem}");
        let mut log = Log::default();
        assert!(matches!(
            driver.print_receipt(&receipt(vec![]), &expected, &mut log),
            Err(Error::NotReady(_))
        ));
        assert_eq!(log.commands, Vec::<&str>::new());
        assert_eq!(fiscal_bodies(&sim), Vec::<String>::new());
    }
}
