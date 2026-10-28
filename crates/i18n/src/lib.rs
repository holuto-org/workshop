//! Language-neutral operator messages backed by embedded Fluent catalogs.
//!
//! UI code must call [`Message::render`] with an explicit language. No selected
//! language is stored globally. [`Display`](fmt::Display) deliberately remains
//! Polish for existing CLI, journal and HTTP diagnostic contracts. UI rendering
//! keeps Fluent's bidirectional isolation; Display uses separate non-isolating
//! bundles, including for nested messages, so persisted identifiers gain no
//! invisible characters. Literal external evidence is never interpreted as FTL.
//!
//! Catalogs are application code: malformed resources, unknown keys and format
//! errors fail loudly rather than returning partial safety instructions or IDs.
//! Diagnostics from those failures never contain interpolation values.

use std::{
    borrow::Cow,
    error::Error,
    fmt, iter,
    sync::{Arc, LazyLock},
};

use fluent_bundle::{FluentArgs, FluentResource, concurrent::FluentBundle};
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;

/// A supported presentation language; never a fiscal or wire-protocol setting.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Language {
    En,
    Pl,
    Es,
    Ru,
}

impl Language {
    pub const ALL: [Self; 4] = [Self::En, Self::Pl, Self::Es, Self::Ru];

    pub const fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Pl => "pl",
            Self::Es => "es",
            Self::Ru => "ru",
        }
    }
}

/// Persisted UI preference. System locales are negotiated anew on resolution.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LanguagePreference {
    #[default]
    System,
    En,
    Pl,
    Es,
    Ru,
}

impl LanguagePreference {
    pub const ALL: [Self; 5] = [Self::System, Self::En, Self::Pl, Self::Es, Self::Ru];

    pub const fn code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::En => "en",
            Self::Pl => "pl",
            Self::Es => "es",
            Self::Ru => "ru",
        }
    }

    pub fn resolve(self) -> Language {
        match self {
            Self::System => self.resolve_from(sys_locale::get_locales()),
            Self::En | Self::Pl | Self::Es | Self::Ru => self.resolve_from(iter::empty::<&str>()),
        }
    }

    /// Honor the first supported OS locale, or English when none are supported.
    /// Region, script, case and Unix encoding/modifier spellings are accepted;
    /// malformed language identifiers are not mistaken for supported prefixes.
    pub fn resolve_from<I, S>(self, locales: I) -> Language
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        match self {
            Self::System => locales
                .into_iter()
                .find_map(|locale| language_from_locale(locale.as_ref()))
                .unwrap_or(Language::En),
            Self::En => Language::En,
            Self::Pl => Language::Pl,
            Self::Es => Language::Es,
            Self::Ru => Language::Ru,
        }
    }
}

fn language_from_locale(locale: &str) -> Option<Language> {
    let identifier = locale
        .trim()
        .split(['.', '@'])
        .next()?
        .parse::<LanguageIdentifier>()
        .ok()?;
    match identifier.language.as_str() {
        "en" => Some(Language::En),
        "pl" => Some(Language::Pl),
        "es" => Some(Language::Es),
        "ru" => Some(Language::Ru),
        _ => None,
    }
}

/// An interpolation value. Integers are stored as exact ungrouped decimal text.
#[derive(Clone, Eq, PartialEq)]
pub enum Argument {
    Text(String),
    Message(Message),
}

impl fmt::Debug for Argument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Argument([redacted])")
    }
}

impl From<String> for Argument {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for Argument {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<&String> for Argument {
    fn from(value: &String) -> Self {
        Self::Text(value.clone())
    }
}

impl From<Message> for Argument {
    fn from(value: Message) -> Self {
        Self::Message(value)
    }
}

macro_rules! integer_arguments {
    ($($integer:ty),+ $(,)?) => {
        $(impl From<$integer> for Argument {
            fn from(value: $integer) -> Self { Self::Text(value.to_string()) }
        })+
    };
}

integer_arguments!(
    u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize
);

/// Deferred presentation text, retaining nested messages across UI switches.
#[derive(Clone, Eq, PartialEq)]
pub struct Message {
    kind: MessageKind,
}

#[derive(Clone, Eq, PartialEq)]
enum MessageKind {
    Catalog {
        id: &'static str,
        arguments: Vec<(&'static str, Argument)>,
    },
    Literal(String),
    Join {
        messages: Vec<Message>,
        separator: &'static str,
    },
}

impl Message {
    pub const fn new(id: &'static str) -> Self {
        Self {
            kind: MessageKind::Catalog {
                id,
                arguments: Vec::new(),
            },
        }
    }

    /// Preserve external OS, server or device detail verbatim, never as Fluent.
    pub fn literal<T: Into<String>>(text: T) -> Self {
        Self {
            kind: MessageKind::Literal(text.into()),
        }
    }

    #[expect(
        clippy::panic,
        reason = "Arguments on non-catalog messages are a programming error."
    )]
    pub fn arg<T: Into<Argument>>(mut self, name: &'static str, value: T) -> Self {
        let MessageKind::Catalog { arguments, .. } = &mut self.kind else {
            panic!("only catalog messages accept arguments");
        };
        let value = value.into();
        if let Some((_, previous)) = arguments.iter_mut().find(|(key, _)| *key == name) {
            *previous = value;
        } else {
            arguments.push((name, value));
        }
        self
    }

    pub fn join<I: IntoIterator<Item = Self>>(messages: I, separator: &'static str) -> Self {
        Self {
            kind: MessageKind::Join {
                messages: messages.into_iter().collect(),
                separator,
            },
        }
    }

    pub fn render(&self, language: Language) -> String {
        match &self.kind {
            MessageKind::Literal(text) => text.clone(),
            MessageKind::Catalog { .. } | MessageKind::Join { .. } => {
                self.render_with(&bundles(language).ui)
            },
        }
    }

    /// Structural emptiness for optional model state, without translating text.
    pub fn is_empty(&self) -> bool {
        match &self.kind {
            MessageKind::Literal(text) => text.is_empty(),
            MessageKind::Catalog { .. } => false,
            MessageKind::Join {
                messages,
                separator,
            } => {
                (messages.len() < 2 || separator.is_empty()) && messages.iter().all(Self::is_empty)
            },
        }
    }

    fn render_with(&self, bundle: &Bundle) -> String {
        let mut output = String::new();
        self.render_into(bundle, &mut output);
        output
    }

    fn render_into(&self, bundle: &Bundle, output: &mut String) {
        match &self.kind {
            MessageKind::Literal(text) => output.push_str(text),
            MessageKind::Catalog { id, arguments } => {
                let mut fluent_arguments = FluentArgs::with_capacity(arguments.len());
                for (name, value) in arguments {
                    match value {
                        Argument::Text(text) => fluent_arguments.set(*name, text.as_str()),
                        Argument::Message(message) => {
                            fluent_arguments.set(*name, message.render_with(bundle));
                        },
                    }
                }
                output.push_str(&format_catalog(bundle, id, Some(&fluent_arguments)));
            },
            MessageKind::Join {
                messages,
                separator,
            } => {
                for (index, message) in messages.iter().enumerate() {
                    if index != 0 {
                        output.push_str(separator);
                    }
                    message.render_into(bundle, output);
                }
            },
        }
    }
}

impl Default for Message {
    fn default() -> Self {
        Self::literal(String::new())
    }
}

impl From<String> for Message {
    fn from(value: String) -> Self {
        Self::literal(value)
    }
}

impl From<&str> for Message {
    fn from(value: &str) -> Self {
        Self::literal(value)
    }
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Message([redacted])")
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            MessageKind::Literal(text) => f.write_str(text),
            MessageKind::Catalog { .. } | MessageKind::Join { .. } => {
                f.write_str(&self.render_with(&bundles(Language::Pl).diagnostic))
            },
        }
    }
}

impl Error for Message {}

/// Render an argument-free dynamic UI key without allocating an owned key.
pub fn text(id: &str, language: Language) -> String {
    format_catalog(&bundles(language).ui, id, None).into_owned()
}

type Bundle = FluentBundle<Arc<FluentResource>>;

struct Bundles {
    ui: Bundle,
    diagnostic: Bundle,
}

fn bundles(language: Language) -> &'static Bundles {
    static EN: LazyLock<Bundles> = LazyLock::new(|| load_bundles(Language::En));
    static PL: LazyLock<Bundles> = LazyLock::new(|| load_bundles(Language::Pl));
    static ES: LazyLock<Bundles> = LazyLock::new(|| load_bundles(Language::Es));
    static RU: LazyLock<Bundles> = LazyLock::new(|| load_bundles(Language::Ru));
    match language {
        Language::En => &EN,
        Language::Pl => &PL,
        Language::Es => &ES,
        Language::Ru => &RU,
    }
}

fn load_bundles(language: Language) -> Bundles {
    let resources = sources(language).map(|(domain, source)| parse_resource(domain, source));
    Bundles {
        ui: make_bundle(language, true, &resources),
        diagnostic: make_bundle(language, false, &resources),
    }
}

#[expect(
    clippy::panic,
    reason = "Embedded catalog syntax is an application defect, never a runtime fallback."
)]
fn parse_resource(domain: &str, source: &str) -> Arc<FluentResource> {
    FluentResource::try_new(source.to_owned()).map_or_else(
        |_invalid_resource| panic!("invalid embedded Fluent resource: {domain}"),
        Arc::new,
    )
}

#[expect(
    clippy::panic,
    reason = "Embedded catalog initialization must not leave partially loaded instructions."
)]
fn make_bundle(language: Language, isolating: bool, resources: &[Arc<FluentResource>]) -> Bundle {
    let Ok(identifier) = language.code().parse() else {
        panic!("invalid embedded language code");
    };
    let mut bundle = Bundle::new_concurrent(vec![identifier]);
    bundle.set_use_isolating(isolating);
    for resource in resources {
        assert!(
            bundle.add_resource(Arc::clone(resource)).is_ok(),
            "conflicting embedded Fluent catalog entries"
        );
    }
    bundle
}

#[expect(
    clippy::panic,
    reason = "Broken catalog references must not leak partial operator instructions or values."
)]
fn format_catalog<'bundle>(
    bundle: &'bundle Bundle,
    id: &str,
    arguments: Option<&FluentArgs<'_>>,
) -> Cow<'bundle, str> {
    let Some(message) = bundle.get_message(id) else {
        panic!("missing embedded Fluent message: {id}");
    };
    let Some(pattern) = message.value() else {
        panic!("embedded Fluent message has no value: {id}");
    };
    let mut errors = Vec::new();
    let rendered = bundle.format_pattern(pattern, arguments, &mut errors);
    assert!(
        errors.is_empty(),
        "embedded Fluent message failed to format: {id}"
    );
    rendered
}

const fn sources(language: Language) -> [(&'static str, &'static str); 4] {
    macro_rules! locale {
        ($language:literal) => {
            [
                (
                    "desktop",
                    include_str!(concat!("../locales/", $language, "/desktop.ftl")),
                ),
                (
                    "agent",
                    include_str!(concat!("../locales/", $language, "/agent.ftl")),
                ),
                (
                    "fiscal",
                    include_str!(concat!("../locales/", $language, "/fiscal.ftl")),
                ),
                (
                    "drivers",
                    include_str!(concat!("../locales/", $language, "/drivers.ftl")),
                ),
            ]
        };
    }
    match language {
        Language::En => locale!("en"),
        Language::Pl => locale!("pl"),
        Language::Es => locale!("es"),
        Language::Ru => locale!("ru"),
    }
}

#[cfg(test)]
mod tests;
