use std::sync::OnceLock;

fn system_is_portuguese() -> bool {
    static IS_PORTUGUESE: OnceLock<bool> = OnceLock::new();
    *IS_PORTUGUESE.get_or_init(|| {
        sys_locale::get_locale().is_some_and(|locale| locale.to_ascii_lowercase().starts_with("pt"))
    })
}

/// Chooses Portuguese for pt-* system locales and English otherwise. English
/// is the deliberate fallback for system languages not yet translated.
pub fn tr(portuguese: &'static str, english: &'static str) -> &'static str {
    if system_is_portuguese() {
        portuguese
    } else {
        english
    }
}

pub fn detected_language_name() -> &'static str {
    if system_is_portuguese() {
        "Português (Brasil)"
    } else {
        "English"
    }
}
