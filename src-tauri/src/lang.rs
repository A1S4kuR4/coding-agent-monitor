//! Shared user-language resolution for user-visible strings (T03).
//!
//! The supported set is Simplified Chinese and English (v0.4 plan §4.5). The
//! default preference is `system`: the tray resolves the Windows UI language
//! once at startup, and unsupported system languages fall back to English. The
//! window resolves the same way in TypeScript (`src/features/usage/i18n.ts`);
//! both sides share the "tag -> supported language" shape and the same fallback
//! rule so the main window and tray never mix languages.

/// A supported UI language for user-visible strings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Language {
    En,
    ZhCn,
}

/// Resolves a BCP-47-ish language tag to the supported set: any `zh*` tag maps
/// to Simplified Chinese; every other (or empty) value falls back to English.
pub fn resolve_language(tag: &str) -> Language {
    if tag.trim().to_ascii_lowercase().starts_with("zh") {
        Language::ZhCn
    } else {
        Language::En
    }
}

#[cfg(windows)]
pub fn system_language() -> Language {
    use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;
    // The UI language (not the regional-format locale) is what user-visible
    // strings should follow. The primary language id is the low 10 bits of the
    // LANGID; 0x0004 covers every Chinese variant (zh-Hans, zh-Hant, ...). The
    // id maps to a tag and resolves through the same rule the TypeScript side
    // uses, so both surfaces share one fallback decision.
    let langid = unsafe { GetUserDefaultUILanguage() };
    let tag = if langid & 0x03FF == 0x0004 {
        "zh"
    } else {
        "en"
    };
    resolve_language(tag)
}

#[cfg(not(windows))]
pub fn system_language() -> Language {
    Language::En
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_tags_map_to_the_chinese_dictionary() {
        assert_eq!(resolve_language("zh-CN"), Language::ZhCn);
        assert_eq!(resolve_language("zh-TW"), Language::ZhCn);
        assert_eq!(resolve_language("zh-Hans-CN"), Language::ZhCn);
        assert_eq!(resolve_language("ZH"), Language::ZhCn);
    }

    #[test]
    fn other_and_missing_tags_fall_back_to_english() {
        assert_eq!(resolve_language("en-US"), Language::En);
        assert_eq!(resolve_language("en"), Language::En);
        assert_eq!(resolve_language("de-DE"), Language::En);
        assert_eq!(resolve_language(""), Language::En);
        assert_eq!(resolve_language("  "), Language::En);
    }
}
