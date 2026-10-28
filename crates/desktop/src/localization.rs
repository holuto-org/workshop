//! Fluent owns both UI labels and operator messages. Language is per component/model,
//! never a process-global locale, so changing it cannot affect fiscal diagnostics.

use i18n::{Language, LanguagePreference};

use crate::ui::Translations;

pub(crate) fn install(global: &Translations<'_>, language: Language) {
    global.on_lookup(|key, index| {
        let language = language_at(index);
        i18n::text(key.as_str(), language).into()
    });
    set_language(global, language);
}

pub(crate) fn set_language(global: &Translations<'_>, language: Language) {
    // The callback's language argument is a Slint property dependency: existing
    // bindings, including tray menus and open dialogs, invalidate when it changes.
    let index = match language {
        Language::En => 0,
        Language::Pl => 1,
        Language::Es => 2,
        Language::Ru => 3,
    };
    global.set_language(index);
}

const fn language_at(index: i32) -> Language {
    match index {
        1 => Language::Pl,
        2 => Language::Es,
        3 => Language::Ru,
        _ => Language::En,
    }
}

pub(crate) fn preference_at(index: i32) -> Option<LanguagePreference> {
    usize::try_from(index)
        .ok()
        .and_then(|index| LanguagePreference::ALL.get(index).copied())
}

pub(crate) const fn preference_index(preference: LanguagePreference) -> i32 {
    match preference {
        LanguagePreference::System => 0,
        LanguagePreference::En => 1,
        LanguagePreference::Pl => 2,
        LanguagePreference::Es => 3,
        LanguagePreference::Ru => 4,
    }
}
