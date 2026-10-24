use super::header_text;

#[test]
fn header_matching_uses_only_the_text_the_printer_displays() {
    assert_eq!(
        header_text("&c&1Auto && Moto&1&c\nŻółw&"),
        "Auto & Moto\nŻółw"
    );
}
