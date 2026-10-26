use std::{collections::BTreeSet, env, error::Error, fs, iter, path::PathBuf};

use quote::{ToTokens, quote};
use schemars::schema::Schema;
use serde_json::{Value, json};
use syn::{File, Item, ItemStruct, Path, Token, parse_quote, parse2, punctuated::Punctuated};
use typify::{TypeSpace, TypeSpacePatch, TypeSpaceSettings};

#[expect(
    clippy::too_many_lines,
    reason = "wire type mapping is one build-time configuration"
)]
fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=../../openapi/agent.json");
    let document: Value = serde_json::from_str(include_str!("../../openapi/agent.json"))?;
    if document.get("openapi") != Some(&json!("3.1.0")) {
        return Err("expected the Rust backend's OpenAPI 3.1 contract".into());
    }
    let mut schemas = document
        .pointer("/components/schemas")
        .and_then(Value::as_object)
        .ok_or("missing components.schemas")?
        .clone();
    // Endpoints return Versioned_Job; the client ignores only its server-side etag.
    let mut versioned_job = schemas
        .get("Versioned_Job")
        .ok_or("missing Versioned_Job")?
        .clone();
    let _etag = versioned_job
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .ok_or("missing versioned job properties")?
        .remove("etag")
        .ok_or("missing versioned job etag")?;
    versioned_job
        .get_mut("required")
        .and_then(Value::as_array_mut)
        .ok_or("missing required versioned job fields")?
        .retain(|field| field != "etag");
    if schemas.get("Job") != Some(&versioned_job) {
        return Err("Versioned_Job no longer matches Job apart from etag".into());
    }
    let mut settings = TypeSpaceSettings::default();
    let _settings = settings
        .with_derive("PartialEq".into())
        .with_derive("Eq".into());
    for (schema, name) in [
        ("VatRate", "VatCode"),
        ("VatRateMapping", "VatRateMapping"),
        ("Printer", "PrinterInfo"),
        ("FiscalJobState", "JobState"),
        ("ReceiptLine", "JobLine"),
        ("AdvanceDeduction", "JobAdvance"),
        ("ReceiptPayment", "JobPayment"),
        ("DeviceQueueForm", "QueueForm"),
        ("CursorPageAgentDocument", "Page"),
        ("AgentPaired", "Pairing"),
    ] {
        let _settings = settings.with_patch(schema, TypeSpacePatch::default().with_rename(name));
    }
    // Validate money/text per job at the fiscal boundary so one bad job can fail alone.
    for name in [
        "AllowedCharacters",
        "Amount",
        "Code",
        "Label",
        "Name",
        "Nip",
        "Note",
        "PositiveAmountAmount",
        "PositiveQuantityQuantity",
        "PositiveUnitPriceUnitPrice",
        "Title",
        "UniqueNumber",
    ] {
        let _settings = settings.with_replacement(name, "String", iter::empty());
    }
    for name in [
        "Attempt",
        "BranchId",
        "FiscalJobId",
        "FiscalPrinterId",
        "PaymentAllocationId",
        "ServiceOrderId",
    ] {
        let _settings = settings.with_replacement(name, "uuid::Uuid", iter::empty());
    }
    for (name, rust) in [
        ("PrinterLimit", "usize"), // Native capability limits; PrinterInfo::check rejects zero.
        ("QuantityDecimals", "u8"),
        ("FiscalVendor", "fiscal::device::Vendor"),
        ("FiscalReceiptKind", "crate::documents::ReceiptKind"),
        ("FiscalPaymentForm", "fiscal::receipt::PaymentForm"),
        ("AgentDocument", "crate::documents::Document"), // Preserve integer-grosze conversion.
        ("TokenTokenKindAgent", "crate::settings::Token"), // Redact credentials.
        ("PairingInput", "String"),                      // Backend validates opaque pairing proofs.
    ] {
        let _settings = settings.with_replacement(name, rust, iter::empty());
    }
    // A native Option override loses required presence; let typify wrap nullable dates.
    let _settings = settings.with_conversion(
        serde_json::from_value(json!({"type":"string","format":"date-time"}))?,
        "jiff::Timestamp",
        iter::empty(),
    );
    // Preserve borrowed versions in outbound fields.
    let telemetry = schemas.get_mut("Telemetry").ok_or("missing Telemetry")?;
    replace_reference(
        telemetry
            .pointer_mut("/properties/appVersion")
            .ok_or("missing appVersion")?,
        "Code",
        "AgentAppVersion",
    )?;
    let _previous = schemas.insert(
        "AgentAppVersion".into(),
        schemas.get("Code").ok_or("missing Code")?.clone(),
    );
    let _settings =
        settings.with_replacement("AgentAppVersion", "super::AppVersion", iter::empty());
    let _settings = settings.with_patch(
        "Telemetry",
        TypeSpacePatch::default().with_attr("#[serde(bound(deserialize = \"'de: 'static\"))]"),
    );
    // Reuse AgentDocument's domain conversion for the identical inline page item.
    let document_schema = schemas
        .get("AgentDocument")
        .ok_or("missing AgentDocument")?
        .clone();
    let item = schemas
        .get_mut("CursorPage_AgentDocument")
        .and_then(|schema| schema.pointer_mut("/properties/items/items"))
        .ok_or("missing document page")?;
    if item != &document_schema {
        return Err("document page item no longer matches AgentDocument".into());
    }
    *item = json!({"$ref":"#/components/schemas/AgentDocument"});
    // Send optional-nullable outbound fields as explicit nulls.
    for (name, fields) in [
        ("ResultForm", &["issuedAt", "receiptNumber", "result"][..]),
        ("DeviceQueueForm", &["allocationId", "buyerNip"][..]),
    ] {
        let required = schemas
            .get_mut(name)
            .and_then(|schema| schema.get_mut("required"))
            .and_then(Value::as_array_mut)
            .ok_or("missing required fields")?;
        required.extend(fields.iter().map(|field| json!(field)));
    }
    let roots = [
        "Telemetry",
        "PrinterStatus",
        "Job",
        "ResultForm",
        "DeviceQueueForm",
        "CursorPage_AgentDocument",
        "AgentPaired",
        "PairForm",
    ];
    let mut needed = BTreeSet::new();
    for root in roots {
        collect(root, &schemas, &mut needed)?;
    }
    let mut definitions = Vec::new();
    for name in needed {
        let mut schema = schemas
            .get(&name)
            .ok_or("missing referenced schema")?
            .clone();
        normalize(&mut schema)?;
        definitions.push((name, serde_json::from_value::<Schema>(schema)?));
    }
    let mut space = TypeSpace::new(&settings);
    space.add_ref_types(definitions)?;
    let mut generated: File = parse2(space.to_stream())?;
    // Move serde attributes after derives to avoid Rust #79202 in typify output.
    let mut debug_impls = Vec::new();
    for item in &mut generated.items {
        if let Item::Struct(item) = item {
            item.attrs
                .sort_by_key(|attribute| attribute.path().is_ident("serde"));
            debug_impls.push(redacted_debug(item)?);
        }
    }
    generated.items.extend(debug_impls);
    let output = PathBuf::from(env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    fs::write(
        output.join("agent-wire.rs"),
        generated.into_token_stream().to_string(),
    )?;
    Ok(())
}

fn collect(
    name: &str,
    schemas: &serde_json::Map<String, Value>,
    needed: &mut BTreeSet<String>,
) -> Result<(), Box<dyn Error>> {
    fn visit(value: &Value, refs: &mut Vec<String>) -> Result<(), Box<dyn Error>> {
        match value {
            Value::Object(object) => {
                if let Some(reference) = object.get("$ref") {
                    refs.push(
                        reference
                            .as_str()
                            .and_then(|value| value.strip_prefix("#/components/schemas/"))
                            .ok_or("unsupported schema reference")?
                            .to_owned(),
                    );
                }
                for value in object.values() {
                    visit(value, refs)?;
                }
            },
            Value::Array(values) => {
                for value in values {
                    visit(value, refs)?;
                }
            },
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {},
        }
        Ok(())
    }
    if !needed.insert(name.to_owned()) {
        return Ok(());
    }
    let mut refs = Vec::new();
    visit(schemas.get(name).ok_or("missing schema")?, &mut refs)?;
    for reference in refs {
        collect(&reference, schemas, needed)?;
    }
    Ok(())
}

fn normalize(value: &mut Value) -> Result<(), Box<dyn Error>> {
    match value {
        Value::Object(object) => {
            if let Some(keyword) = [
                "$dynamicRef",
                "$dynamicAnchor",
                "prefixItems",
                "unevaluatedItems",
                "unevaluatedProperties",
                "dependentSchemas",
                "dependentRequired",
                "contains",
                "minContains",
                "maxContains",
            ]
            .into_iter()
            .find(|keyword| object.contains_key(*keyword))
            {
                return Err(format!("schema keyword unsupported by typify 0.8: {keyword}").into());
            }
            if let Some(reference) = object.get_mut("$ref") {
                let name = reference
                    .as_str()
                    .and_then(|reference| reference.strip_prefix("#/components/schemas/"))
                    .ok_or("unsupported schema reference")?;
                *reference = json!(format!("#/definitions/{name}"));
            }
            // Allow new response fields; the fake still enforces the full schema.
            if object.get("additionalProperties") == Some(&Value::Bool(false)) {
                let _previous = object.remove("additionalProperties");
            }
            for value in object.values_mut() {
                normalize(value)?;
            }
        },
        Value::Array(values) => {
            for value in values {
                normalize(value)?;
            }
        },
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {},
    }
    Ok(())
}

fn replace_reference(
    value: &mut Value,
    expected: &str,
    replacement: &str,
) -> Result<(), Box<dyn Error>> {
    if value != &json!({"$ref": format!("#/components/schemas/{expected}")}) {
        return Err(format!("expected schema reference to {expected}").into());
    }
    *value = json!({"$ref": format!("#/components/schemas/{replacement}")});
    Ok(())
}

fn redacted_debug(item: &mut ItemStruct) -> Result<Item, Box<dyn Error>> {
    let mut removed = false;
    for attribute in &mut item.attrs {
        if !attribute.path().is_ident("derive") {
            continue;
        }
        let derives = attribute.parse_args_with(Punctuated::<Path, Token![,]>::parse_terminated)?;
        let filtered: Punctuated<Path, Token![,]> = derives
            .into_iter()
            .filter(|derive| {
                if derive
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "Debug")
                {
                    removed = true;
                    false
                } else {
                    true
                }
            })
            .collect();
        attribute.meta = parse_quote!(derive(#filtered));
    }
    if !removed {
        return Err("expected typify's wire struct Debug derive".into());
    }
    let name = &item.ident;
    let (implementation, arguments, constraints) = item.generics.split_for_impl();
    parse2(quote! {
        impl #implementation ::std::fmt::Debug for #name #arguments #constraints {
            fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                formatter.debug_struct(stringify!(#name)).finish_non_exhaustive()
            }
        }
    })
    .map_err(Into::into)
}
