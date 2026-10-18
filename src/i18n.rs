//! Minimal two-language support (English / Russian).
//! Usage: `tr!("English text", "Русский текст")` — both arms may be `format!(..)`.

use std::sync::atomic::{AtomicBool, Ordering};

static RUSSIAN: AtomicBool = AtomicBool::new(false);

pub fn is_ru() -> bool {
    RUSSIAN.load(Ordering::Relaxed)
}

/// `pref` is the config value: "en", "ru" or None for auto-detection from the locale.
pub fn set_language(pref: Option<&str>) {
    let ru = match pref {
        Some(lang) => lang.starts_with("ru"),
        None => system_is_ru(),
    };
    RUSSIAN.store(ru, Ordering::Relaxed);
}

fn system_is_ru() -> bool {
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() {
                return v.starts_with("ru");
            }
        }
    }
    false
}

#[macro_export]
macro_rules! tr {
    ($en:expr, $ru:expr $(,)?) => {
        if $crate::i18n::is_ru() { $ru } else { $en }
    };
}
