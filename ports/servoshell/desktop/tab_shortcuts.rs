#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TabShortcut {
    Search,
    Reopen,
}

pub(crate) fn tab_shortcut(
    control: bool,
    shift: bool,
    alt: bool,
    super_key: bool,
    key: &str,
) -> Option<TabShortcut> {
    if !control || !shift || alt || super_key {
        return None;
    }
    if key.eq_ignore_ascii_case("a") {
        Some(TabShortcut::Search)
    } else if key.eq_ignore_ascii_case("t") {
        Some(TabShortcut::Reopen)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn global_chords_are_independent_of_chrome_input_focus() {
        assert_eq!(
            tab_shortcut(true, true, false, false, "A"),
            Some(TabShortcut::Search)
        );
        assert_eq!(
            tab_shortcut(true, true, false, false, "t"),
            Some(TabShortcut::Reopen)
        );
    }
    #[test]
    fn ordinary_editing_and_other_modifier_chords_are_untouched() {
        assert_eq!(tab_shortcut(true, false, false, false, "a"), None);
        assert_eq!(tab_shortcut(true, false, false, false, "t"), None);
        assert_eq!(tab_shortcut(true, true, true, false, "a"), None);
        assert_eq!(tab_shortcut(false, true, false, false, "t"), None);
        assert_eq!(tab_shortcut(true, true, false, true, "t"), None);
    }
}
