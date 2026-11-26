//! Idioma de la interfaz: español (por defecto) o inglés.

use std::sync::atomic::{AtomicBool, Ordering};

static EN: AtomicBool = AtomicBool::new(false);

pub fn set_lang(lang: &str) {
    EN.store(lang == "en", Ordering::Relaxed);
}

pub fn en() -> bool {
    EN.load(Ordering::Relaxed)
}

/// Texto fijo en el idioma activo.
pub fn tr(es: &'static str, en_: &'static str) -> &'static str {
    if en() { en_ } else { es }
}

/// Como `format!`, con una versión por idioma.
#[macro_export]
macro_rules! trf {
    ($es:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::en() { format!($en $(, $arg)*) } else { format!($es $(, $arg)*) }
    };
}
