//! Posnet frames: `STX command TAB (key value TAB)* # CRC16 ETX`, payload in Windows-1250.
//!
//! CRC-16/XMODEM (polynomial 0x1021, initial value 0) covers the payload;
//! four uppercase hex digits carry the checksum (spec p. 13).

use fiscal::{cp1250, error::Error};

pub(crate) const MAX_FRAME: usize = 16_384;
pub(crate) const STX: u8 = 0x02;
pub(crate) const ETX: u8 = 0x03;
/// Two-letter keys, except `@` and the `?` error code.
const KEY_LENGTH: usize = 2;
const CORRUPT: &str = "Uszkodzona odpowiedź drukarki (CRC lub kodowanie).";

/// Reply parameters in device order, with unique keys.
#[derive(Debug)]
pub struct Reply(Vec<(String, String)>, Option<u32>);

impl Reply {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    pub(crate) fn require(&self, key: &str) -> Result<&str, Error> {
        self.get(key).ok_or_else(|| {
            Error::Protocol(format!(
                "Niepełna odpowiedź drukarki: brak parametru {key}."
            ))
        })
    }

    pub fn fields(&self) -> &[(String, String)] {
        &self.0
    }

    /// Correlate the envelope (including `@`) before attributing a device error.
    pub(crate) fn into_result(self, command: &str) -> Result<Self, Error> {
        if let Some(code) = self.1 {
            let named = self.get("cm").unwrap_or(command);
            return Err(Error::Device {
                code,
                command: named.to_owned(),
                detail: format!(
                    "Drukarka zwróciła błąd {code} ({named}{}).{}",
                    parameter(self.get("fd").unwrap_or_default()),
                    explanation(code),
                ),
                rejected: command == "ERR",
            });
        }
        Ok(self)
    }
}

pub(crate) fn crc16(payload: &[u8]) -> u16 {
    payload.iter().fold(0, |crc, byte| {
        (0..8).fold(crc ^ (u16::from(*byte) << 8), |crc, _| {
            if crc & 0x8000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x1021
            }
        })
    })
}

pub(crate) fn wrap(payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 7);
    frame.push(STX);
    frame.extend_from_slice(payload);
    frame.extend_from_slice(format!("#{:04X}", crc16(payload)).as_bytes());
    frame.push(ETX);
    frame
}

fn valid_command(command: &str) -> bool {
    let name = command.strip_prefix('!').unwrap_or(command);
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && characters.all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
}

fn valid_key(key: &str) -> bool {
    key == "@"
        || (key.len() == KEY_LENGTH && key.chars().all(|character| character.is_ascii_lowercase()))
}

/// Values must be printable Windows-1250 without tabs or controls.
pub(crate) fn encode(command: &str, params: &[(&str, String)]) -> Result<Vec<u8>, Error> {
    if !valid_command(command) {
        return Err(Error::Invalid(
            "Nieprawidłowe polecenie drukarki.".to_owned(),
        ));
    }
    let mut payload = String::from(command);
    payload.push('\t');
    for (key, value) in params {
        if !valid_key(key) {
            return Err(Error::Invalid(
                "Nieprawidłowy parametr drukarki.".to_owned(),
            ));
        }
        check_printable(value)?;
        payload.push_str(key);
        payload.push_str(value);
        payload.push('\t');
    }
    let encoded = cp1250::encode(&payload).ok_or_else(|| {
        Error::Invalid("Drukarka nie obsługuje niektórych znaków w tekście.".to_owned())
    })?;
    let frame = wrap(&encoded);
    if frame.len() > MAX_FRAME {
        return Err(Error::Invalid(
            "Polecenie drukarki jest zbyt długie.".to_owned(),
        ));
    }
    Ok(frame)
}

#[cfg(any(test, feature = "simulator"))]
/// Error codes yield [`Error::Device`]; `ERR` without a code is a protocol error.
pub(crate) fn decode(frame: &[u8]) -> Result<(String, Reply), Error> {
    let (command, reply) = decode_response(frame)?;
    reply.into_result(&command).map(|reply| (command, reply))
}

pub(crate) fn decode_response(frame: &[u8]) -> Result<(String, Reply), Error> {
    let body = frame
        .strip_prefix(&[STX])
        .and_then(|rest| rest.strip_suffix(&[ETX]))
        .filter(|_| frame.len() <= MAX_FRAME)
        .ok_or_else(|| Error::Protocol("Nieprawidłowa ramka odpowiedzi drukarki.".to_owned()))?;
    let separator = body
        .iter()
        .rposition(|byte| *byte == b'#')
        .ok_or_else(corrupt)?;
    let (payload, checksum) = body.split_at(separator);
    let checksum = checksum.get(1..).ok_or_else(corrupt)?;
    let expected = str::from_utf8(checksum)
        .ok()
        .filter(|digits| digits.len() == 4 && digits.chars().all(|digit| digit.is_ascii_hexdigit()))
        .and_then(|digits| u16::from_str_radix(digits, 16).ok())
        .ok_or_else(corrupt)?;
    if crc16(payload) != expected {
        return Err(corrupt());
    }
    let text = cp1250::decode(payload).ok_or_else(corrupt)?;
    let mut fields = text.split('\t');
    let command = fields.next().unwrap_or_default().to_owned();
    let mut reply = Vec::new();
    let mut error = None;
    for field in fields.filter(|field| !field.is_empty()) {
        if let Some(code) = field.strip_prefix('?') {
            if error.is_some() {
                return Err(Error::Protocol(
                    "Powtórzony kod błędu w odpowiedzi drukarki.".to_owned(),
                ));
            }
            error = Some(code.parse::<u32>().map_err(|parse| {
                Error::Protocol(format!("Nieprawidłowy kod błędu drukarki: {parse}."))
            })?);
        } else {
            let (key, value) = if let Some(value) = field.strip_prefix('@') {
                ("@", value)
            } else {
                let split = field.char_indices().nth(KEY_LENGTH - 1);
                let split = split.map(|(index, character)| index + character.len_utf8());
                let Some(pair) = split.map(|index| field.split_at(index)) else {
                    return Err(Error::Protocol(
                        "Niepełny parametr odpowiedzi drukarki.".to_owned(),
                    ));
                };
                pair
            };
            if reply.iter().any(|(name, _)| name == key) {
                return Err(Error::Protocol(
                    "Powtórzony parametr w odpowiedzi drukarki.".to_owned(),
                ));
            }
            reply.push((key.to_owned(), value.to_owned()));
        }
    }
    let reply = Reply(reply, error);
    if command == "ERR" && error.is_none() {
        return Err(Error::Protocol(
            "Drukarka odrzuciła ramkę bez kodu błędu.".to_owned(),
        ));
    }
    Ok((command, reply))
}

fn corrupt() -> Error {
    Error::Protocol(CORRUPT.to_owned())
}

fn check_printable(value: &str) -> Result<(), Error> {
    if value.chars().any(char::is_control) {
        return Err(Error::Invalid(
            "Tekst nie może zawierać znaków sterujących.".to_owned(),
        ));
    }
    if cp1250::encode(value).is_none() {
        return Err(Error::Invalid(
            "Drukarka nie obsługuje niektórych znaków w tekście.".to_owned(),
        ));
    }
    Ok(())
}

fn parameter(field: &str) -> String {
    if field.is_empty() {
        String::new()
    } else {
        format!(", parametr {field}")
    }
}

/// Errors preflight cannot detect (POT-I-DEV-37 v5978, "Opisy błędów").
const fn explanation(code: u32) -> &'static str {
    match code {
        382 => " Nie można wykonać dwóch kolejnych zerowych raportów dobowych.",
        383 => " Brak raportu dobowego za poprzedni dzień sprzedaży. Wykonaj raport dobowy.",
        387 => " Sprzedaż zablokowana. Brak miejsca w pamięci.",
        484 => " Minął czas pracy kasy, sprzedaż zablokowana.",
        488 => " Subskrypcja wygasła, sprzedaż zablokowana.",
        1039 | 2904 => " Pamięć chroniona zapełniona.",
        2036 => " Urządzenie w stanie tylko do odczytu.",
        2098 => " Pamięć fiskalna zapełniona.",
        // Selling a name at a higher rate after a lower one (p. 296).
        2106 => {
            " Towar zablokowany: tę nazwę sprzedano wcześniej w niższej stawce VAT, a drukarka \
             nie pozwala jej podwyższyć. Zmień nazwę pozycji."
        },
        3728 => " Upłynął termin ważności certyfikatu.",
        3733 => " Sprzedaż zablokowana. Brak przekazanych kluczy do Repozytorium.",
        3735 => " Sprzedaż zablokowana. Wgraj nowe certyfikaty.",
        3903 => " Operacja w toku. Spróbuj ponownie.",
        3929 => " Sprzedaż zablokowana. Brak klucza współdzielonego.",
        _ => "",
    }
}

#[cfg(test)]
pub(crate) mod tests;
