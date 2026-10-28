use std::{
    collections::{BTreeMap, BTreeSet},
    io::{self, ErrorKind},
    iter::from_fn,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::Barrier,
    thread::Builder,
};

use fluent_syntax::ast::{
    CallArguments, Entry, Expression, InlineExpression, Pattern, PatternElement,
};

use super::*;

#[test]
fn polish_diagnostic_display_preserves_nested_uncertainty_and_exact_identifiers() {
    let message = Message::new("fiscal-uncertain").arg(
        "detail",
        Message::new("fiscal-timeout").arg("command", "trinit"),
    );
    assert_eq!(
        message.to_string(),
        concat!(
            "Brak odpowiedzi na trinit. Polecenie nie zostało ponowione.\n",
            "Wynik operacji wymaga sprawdzenia. ",
            "Nie wysyłaj jej ponownie przed sprawdzeniem drukarki."
        )
    );
    let english = message.render(Language::En);
    assert_ne!(english, message.to_string());
    assert!(
        english.contains("trinit"),
        "nested protocol identifier lost"
    );
}

#[test]
fn io_errors_retain_localizable_structured_detail() {
    let message = Message::new("fiscal-timeout").arg("command", "trinit");
    let error = io::Error::new(ErrorKind::InvalidData, message.clone());
    assert_eq!(
        error
            .get_ref()
            .and_then(|source| source.downcast_ref::<Message>()),
        Some(&message)
    );
    assert_eq!(
        error.to_string(),
        "Brak odpowiedzi na trinit. Polecenie nie zostało ponowione."
    );
}

#[test]
fn dynamic_ui_keys_render_without_requiring_static_storage() {
    let key = ["fiscal", "report", "daily"].join("-");
    assert_eq!(text(&key, Language::Pl), "Raport dobowy");
    assert_eq!(
        Message::new("fiscal-report-daily").render(Language::Pl),
        "Raport dobowy"
    );
}

#[test]
fn system_negotiates_order_normalization_and_fallback() {
    let cases: &[(&[&str], Language)] = &[
        (&[], Language::En),
        (&["de-DE", "PL_pl.UTF-8", "es"], Language::Pl),
        (&[" ES_ar@euro ", "ru"], Language::Es),
        (&["RU_cyrl_RU.UTF-8"], Language::Ru),
        (&["en-US", "pl"], Language::En),
        (&["fr", "C.UTF-8", "POSIX", ""], Language::En),
        (&["pl-not-a-valid-region", "ru-RU"], Language::Ru),
        (&["english", "polish", "russian", "spanish"], Language::En),
    ];
    for &(locales, expected) in cases {
        assert_eq!(
            LanguagePreference::System.resolve_from(locales),
            expected,
            "{locales:?}"
        );
    }
}

#[test]
fn explicit_preference_never_consults_system_candidates() {
    let choices = [
        (LanguagePreference::En, Language::En),
        (LanguagePreference::Pl, Language::Pl),
        (LanguagePreference::Es, Language::Es),
        (LanguagePreference::Ru, Language::Ru),
    ];
    for (preference, expected) in choices {
        assert_eq!(preference.resolve_from(["de", "pl", "es"]), expected);
        let never =
            from_fn(|| -> Option<&str> { panic!("explicit preference examined OS locales") });
        assert_eq!(preference.resolve_from(never), expected);
    }
}

#[test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions check the settings serialization contract while serde failures propagate."
)]
fn preferences_round_trip_with_stable_lowercase_settings_values() -> Result<(), serde_json::Error> {
    for (preference, serialized) in LanguagePreference::ALL.into_iter().zip([
        "\"system\"",
        "\"en\"",
        "\"pl\"",
        "\"es\"",
        "\"ru\"",
    ]) {
        assert_eq!(serde_json::to_string(&preference)?, serialized);
        assert_eq!(
            serde_json::from_str::<LanguagePreference>(serialized)?,
            preference
        );
    }
    assert_eq!(LanguagePreference::default(), LanguagePreference::System);
    assert!(serde_json::from_str::<LanguagePreference>("\"de\"").is_err());
    Ok(())
}

fn fixture(language: Language, isolating: bool) -> Bundle {
    let source = match language {
        Language::En => {
            "inner = Warning { $id }\nouter = Detail: { $detail }\nnumber = ID { $id }\n"
        },
        Language::Pl => {
            "inner = Uwaga { $id }\nouter = Szczegóły: { $detail }\nnumber = ID { $id }\n"
        },
        Language::Es => {
            "inner = Aviso { $id }\nouter = Detalle: { $detail }\nnumber = ID { $id }\n"
        },
        Language::Ru => {
            "inner = Внимание { $id }\nouter = Подробности: { $detail }\nnumber = ID { $id }\n"
        },
    };
    make_bundle(language, isolating, &[parse_resource("test", source)])
}

#[test]
fn nested_messages_retranslate_without_losing_literal_device_evidence() {
    let nested = Message::new("outer").arg(
        "detail",
        Message::join(
            [
                Message::new("inner").arg("id", "00012"),
                Message::literal("device: الخطأ / Żółć / { $not_fluent }"),
            ],
            " | ",
        ),
    );
    for (language, expected) in [
        (
            Language::En,
            "Detail: Warning 00012 | device: الخطأ / Żółć / { $not_fluent }",
        ),
        (
            Language::Pl,
            "Szczegóły: Uwaga 00012 | device: الخطأ / Żółć / { $not_fluent }",
        ),
        (
            Language::Es,
            "Detalle: Aviso 00012 | device: الخطأ / Żółć / { $not_fluent }",
        ),
        (
            Language::Ru,
            "Подробности: Внимание 00012 | device: الخطأ / Żółć / { $not_fluent }",
        ),
    ] {
        assert_eq!(nested.render_with(&fixture(language, false)), expected);
    }
}

#[test]
fn integers_never_round_or_gain_locale_grouping() {
    for language in Language::ALL {
        let bundle = fixture(language, false);
        assert_eq!(
            Message::new("number")
                .arg("id", u128::MAX)
                .render_with(&bundle),
            "ID 340282366920938463463374607431768211455"
        );
        assert_eq!(
            Message::new("number")
                .arg("id", i128::MIN)
                .render_with(&bundle),
            "ID -170141183460469231731687303715884105728"
        );
        assert_eq!(
            Message::new("number")
                .arg("id", 0_usize)
                .render_with(&bundle),
            "ID 0"
        );
    }
}

#[test]
fn ui_isolates_interpolated_evidence_but_legacy_rendering_does_not() {
    let message = Message::new("number").arg("id", "אב12");
    assert_eq!(
        message.render_with(&fixture(Language::Pl, true)),
        "ID \u{2068}אב12\u{2069}"
    );
    assert_eq!(
        message.render_with(&fixture(Language::Pl, false)),
        "ID אב12"
    );
    assert_eq!(Message::literal("אב12").to_string(), "אב12");
}

#[test]
fn repeated_argument_names_replace_previous_values() {
    let message = Message::new("number").arg("id", "old").arg("id", "new");
    assert_eq!(message.render_with(&fixture(Language::En, false)), "ID new");
}

#[test]
fn debug_never_discloses_literals_nested_arguments_or_separators() {
    let secret = "sensitive-device-token";
    let message = Message::join(
        [
            Message::literal(secret),
            Message::new("outer").arg("detail", Message::new("inner").arg("id", secret)),
        ],
        "sensitive-separator",
    );
    assert_eq!(format!("{message:?}"), "Message([redacted])");
    assert_eq!(
        format!("{:?}", Argument::from(secret)),
        "Argument([redacted])"
    );
}

#[test]
fn join_preserves_order_empty_parts_and_structural_empty_state() {
    assert!(Message::join([], ", ").is_empty());
    assert!(Message::default().is_empty());
    assert!(Message::join([Message::default()], ", ").is_empty());
    let message = Message::join(
        [
            Message::literal("a"),
            Message::default(),
            Message::literal("b"),
        ],
        " / ",
    );
    assert_eq!(message.render(Language::En), "a /  / b");
    assert!(!message.is_empty());
    assert!(!Message::join([Message::default(), Message::default()], " ").is_empty());
    assert!(Message::join([Message::default(), Message::default()], "").is_empty());
}

#[test]
fn unknown_messages_and_missing_arguments_never_escape_as_partial_output() {
    let bundle = fixture(Language::En, true);
    for message in [Message::new("missing"), Message::new("outer")] {
        assert!(catch_unwind(AssertUnwindSafe(|| message.render_with(&bundle))).is_err());
    }
}

#[test]
fn bundles_are_reused_and_safe_for_concurrent_rendering() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Message>();
    assert_send_sync::<Argument>();
    assert_send_sync::<Bundles>();
    for language in Language::ALL {
        let gate = Arc::new(Barrier::new(4));
        let mut handles = Vec::with_capacity(4);
        for index in 0..4 {
            let gate = Arc::clone(&gate);
            let handle = Builder::new()
                .name(format!("i18n-{}-{index}", language.code()))
                .spawn(move || {
                    let _leader = gate.wait();
                    assert!(
                        ptr::eq(bundles(language), bundles(language)),
                        "catalog must be cached"
                    );
                    let message = Message::new("fiscal-timeout").arg("command", "trinit");
                    for _ in 0..32 {
                        assert!(
                            message.render(language).contains("trinit"),
                            "concurrent rendering lost protocol identifier"
                        );
                    }
                })
                .unwrap_or_else(|error| panic!("cannot start catalog rendering thread: {error}"));
            handles.push(handle);
        }
        let mut all_succeeded = true;
        for handle in handles {
            all_succeeded &= handle.join().is_ok();
        }
        assert!(
            all_succeeded,
            "concurrent catalog initialization/rendering failed"
        );
    }
}

#[derive(Default)]
struct PatternInfo<'a> {
    variables: BTreeSet<&'a str>,
    references: BTreeSet<String>,
    patterns: Vec<&'a Pattern<&'a str>>,
}

fn inspect_pattern<'a>(pattern: &'a Pattern<&'a str>, info: &mut PatternInfo<'a>) {
    info.patterns.push(pattern);
    for element in &pattern.elements {
        if let PatternElement::Placeable { expression } = element {
            inspect_expression(expression, info);
        }
    }
}

fn inspect_expression<'a>(expression: &'a Expression<&'a str>, info: &mut PatternInfo<'a>) {
    match expression {
        Expression::Inline(inline) => inspect_inline(inline, info),
        Expression::Select { selector, variants } => {
            inspect_inline(selector, info);
            for variant in variants {
                inspect_pattern(&variant.value, info);
            }
        },
    }
}

fn inspect_arguments<'a>(arguments: &'a CallArguments<&'a str>, info: &mut PatternInfo<'a>) {
    for argument in &arguments.positional {
        inspect_inline(argument, info);
    }
    for argument in &arguments.named {
        inspect_inline(&argument.value, info);
    }
}

fn inspect_inline<'a>(inline: &'a InlineExpression<&'a str>, info: &mut PatternInfo<'a>) {
    match inline {
        InlineExpression::VariableReference { id } => {
            let _inserted = info.variables.insert(id.name);
        },
        InlineExpression::MessageReference { id, attribute } => {
            let _inserted = info.references.insert(reference_key(
                "",
                id.name,
                attribute.as_ref().map(|attribute| attribute.name),
            ));
        },
        InlineExpression::TermReference {
            id,
            attribute,
            arguments,
        } => {
            let _inserted = info.references.insert(reference_key(
                "-",
                id.name,
                attribute.as_ref().map(|attribute| attribute.name),
            ));
            if let Some(arguments) = arguments {
                inspect_arguments(arguments, info);
            }
        },
        InlineExpression::FunctionReference { arguments, .. } => inspect_arguments(arguments, info),
        InlineExpression::Placeable { expression } => inspect_expression(expression, info),
        InlineExpression::StringLiteral { .. } | InlineExpression::NumberLiteral { .. } => {},
    }
}

fn reference_key(prefix: &str, id: &str, attribute: Option<&str>) -> String {
    attribute.map_or_else(
        || format!("{prefix}{id}"),
        |attribute| format!("{prefix}{id}.{attribute}"),
    )
}

#[test]
fn every_embedded_catalog_has_matching_ids_variables_and_resolvable_patterns() {
    let mut baseline = None;
    for language in Language::ALL {
        let resources: Vec<_> = sources(language)
            .into_iter()
            .map(|(domain, source)| (domain, parse_resource(domain, source)))
            .collect();
        let bundle_resources: Vec<_> = resources
            .iter()
            .map(|(_, resource)| Arc::clone(resource))
            .collect();
        let bundle = make_bundle(language, true, &bundle_resources);
        let mut entries = BTreeMap::new();
        for (domain, resource) in &resources {
            let mut domain_messages = 0;
            for entry in resource.entries() {
                let (prefix, id, value, attributes) = match entry {
                    Entry::Message(message) => {
                        domain_messages += 1;
                        assert!(
                            message.id.name.starts_with(&format!("{domain}-")),
                            "wrong domain: {language:?}/{}",
                            message.id.name
                        );
                        (
                            "",
                            message.id.name,
                            message.value.as_ref(),
                            &message.attributes,
                        )
                    },
                    Entry::Term(term) => ("-", term.id.name, Some(&term.value), &term.attributes),
                    Entry::Comment(_) | Entry::GroupComment(_) | Entry::ResourceComment(_) => {
                        continue;
                    },
                    Entry::Junk { .. } => panic!("junk in embedded {language:?}/{domain} catalog"),
                };
                assert!(
                    value.is_some(),
                    "messages need a renderable value: {language:?}/{id}"
                );
                if let Some(value) = value {
                    let mut info = PatternInfo::default();
                    inspect_pattern(value, &mut info);
                    assert!(
                        entries
                            .insert(reference_key(prefix, id, None), info)
                            .is_none(),
                        "duplicate {id}"
                    );
                }
                for attribute in attributes {
                    let mut info = PatternInfo::default();
                    inspect_pattern(&attribute.value, &mut info);
                    assert!(
                        entries
                            .insert(reference_key(prefix, id, Some(attribute.id.name)), info)
                            .is_none(),
                        "duplicate attribute on {id}"
                    );
                }
            }
            assert!(domain_messages > 0, "empty {language:?}/{domain} catalog");
        }
        let schema: BTreeMap<_, BTreeSet<String>> = entries
            .iter()
            .map(|(id, info)| {
                (
                    id.clone(),
                    info.variables
                        .iter()
                        .map(|name| (*name).to_owned())
                        .collect(),
                )
            })
            .collect();
        if let Some(baseline) = &baseline {
            assert_eq!(
                &schema, baseline,
                "catalog IDs or argument names diverged for {language:?}"
            );
        } else {
            baseline = Some(schema);
        }
        check_patterns(language, &bundle, &entries);
    }
}

fn check_patterns(
    language: Language,
    bundle: &Bundle,
    entries: &BTreeMap<String, PatternInfo<'_>>,
) {
    let mut arguments = FluentArgs::new();
    for info in entries.values() {
        for variable in &info.variables {
            arguments.set(*variable, "sample-00042");
        }
    }
    for (id, info) in entries {
        for reference in &info.references {
            assert!(
                entries.contains_key(reference),
                "unresolved reference {reference} in {language:?}/{id}"
            );
        }
        for pattern in &info.patterns {
            let mut errors = Vec::new();
            let rendered = bundle.format_pattern(pattern, Some(&arguments), &mut errors);
            assert!(
                errors.is_empty(),
                "broken pattern in {language:?}/{id}: {errors:?}"
            );
            assert!(
                !rendered.trim().is_empty(),
                "empty pattern in {language:?}/{id}"
            );
        }
    }
}

#[test]
fn recovery_instructions_name_the_action_the_operator_can_actually_choose() {
    for language in Language::ALL {
        for (notice, action) in [
            (
                Message::new("agent-report-interrupted"),
                "desktop-resolve-report",
            ),
            (
                Message::new("agent-orphan-first"),
                "desktop-resolve-receipt",
            ),
            (
                Message::new("agent-orphan-evidence")
                    .arg("number", "41")
                    .arg("order", "10")
                    .arg("job", "job-42")
                    .arg("state", "Failed"),
                "desktop-resolve-receipt",
            ),
        ] {
            let instruction = notice.render(language);
            let label = text(action, language);
            assert!(
                instruction.contains(&label),
                "{language:?}: {instruction} names no {label}"
            );
        }
    }
}
