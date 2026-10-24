use super::Vendor;

#[test]
fn vendor_serializes_snake_case() {
    for (vendor, name) in [
        (Vendor::Posnet, "\"posnet\""),
        (Vendor::Novitus, "\"novitus\""),
        (Vendor::Elzab, "\"elzab\""),
    ] {
        assert_eq!(serde_json::to_string(&vendor).unwrap(), name);
        assert_eq!(serde_json::from_str::<Vendor>(name).unwrap(), vendor);
    }
}
