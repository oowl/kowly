//! English or Chinese, for what the engine says: errors, notifications and
//! transfer details. The window sets the language; until it does, it
//! follows the locale.

use std::sync::atomic::{AtomicBool, Ordering};

static CHINESE: AtomicBool = AtomicBool::new(false);

pub fn chinese() -> bool {
    CHINESE.load(Ordering::Relaxed)
}

pub fn set_chinese(on: bool) {
    CHINESE.store(on, Ordering::Relaxed);
}

/// Whether the locale is Chinese, by the variables gettext reads, in its
/// order.
pub fn locale_is_chinese() -> bool {
    ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|v| std::env::var(v).ok())
        .find(|v| !v.is_empty())
        .is_some_and(|v| v.starts_with("zh"))
}

/// `format!` in the current language: `tr!("{n} files", "{n} 个文件")`.
macro_rules! tr {
    ($en:literal, $zh:literal $(, $arg:expr)* $(,)?) => {
        if $crate::airdrop::i18n::chinese() {
            format!($zh $(, $arg)*)
        } else {
            format!($en $(, $arg)*)
        }
    };
}

pub(crate) use tr;
