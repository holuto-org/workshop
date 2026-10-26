/// Header escapes: `&&` prints `&`; other `&x` pairs are formatting marks.
pub(crate) fn header_text(raw: &str) -> String {
    let mut text = String::with_capacity(raw.len());
    let mut characters = raw.chars();
    while let Some(character) = characters.next() {
        if character != '&' {
            text.push(character);
        } else if characters.next() == Some('&') {
            text.push('&');
        }
    }
    text
}
