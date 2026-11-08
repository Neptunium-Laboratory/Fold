//! Keyboard input.
//!
//! This file only translates key presses into `Action`s. What an action *does*
//! lives in `app.rs`. To change a key binding, edit `normal_action` below.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Quit,
    Up,
    Down,
    PageUp,
    PageDown,
    Top,
    Bottom,
    /// Enter: open the folder or file under the cursor.
    Open,
    /// Right / l: go into a folder (never launches files).
    Descend,
    Parent,
    ToggleSelect,
    SelectAll,
    Copy,
    Move,
    Rename,
    Delete,
    Paste,
    FilterStart,
    Escape,
    ToggleHidden,
    Help,
    Settings,
    BookmarkAdd,
    BookmarksOpen,
}

/// Ctrl+C always quits, whatever is on screen.
pub fn is_interrupt(key: &KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
}

/// Key bindings for the normal (file list) mode.
pub fn normal_action(key: KeyEvent) -> Option<Action> {
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return None;
    }
    Some(match key.code {
        KeyCode::Up | KeyCode::Char('k') => Action::Up,
        KeyCode::Down | KeyCode::Char('j') => Action::Down,
        KeyCode::PageUp => Action::PageUp,
        KeyCode::PageDown => Action::PageDown,
        KeyCode::Home | KeyCode::Char('g') => Action::Top,
        KeyCode::End | KeyCode::Char('G') => Action::Bottom,

        KeyCode::Enter => Action::Open,
        KeyCode::Right | KeyCode::Char('l') => Action::Descend,
        KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => Action::Parent,

        KeyCode::Char(' ') => Action::ToggleSelect,
        KeyCode::Char('a') => Action::SelectAll,

        KeyCode::Char('c') | KeyCode::Char('C') => Action::Copy,
        KeyCode::Char('m') | KeyCode::Char('M') => Action::Move,
        KeyCode::Char('r') | KeyCode::Char('R') => Action::Rename,
        // Deleting always goes through a confirmation dialog.
        KeyCode::Char('d') | KeyCode::Char('D') | KeyCode::Delete => Action::Delete,
        KeyCode::Char('p') | KeyCode::Char('P') => Action::Paste,

        KeyCode::Char('/') => Action::FilterStart,
        KeyCode::Esc => Action::Escape,
        KeyCode::Char('.') => Action::ToggleHidden,
        KeyCode::Char('?') => Action::Help,
        KeyCode::Char('s') | KeyCode::Char('S') => Action::Settings,
        KeyCode::Char('b') => Action::BookmarkAdd,
        KeyCode::Char('B') => Action::BookmarksOpen,
        KeyCode::Char('q') | KeyCode::Char('Q') => Action::Quit,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn brief_bindings() {
        assert_eq!(normal_action(key(KeyCode::Up)), Some(Action::Up));
        assert_eq!(normal_action(key(KeyCode::Down)), Some(Action::Down));
        assert_eq!(normal_action(key(KeyCode::Enter)), Some(Action::Open));
        assert_eq!(normal_action(key(KeyCode::Backspace)), Some(Action::Parent));
        assert_eq!(
            normal_action(key(KeyCode::Char(' '))),
            Some(Action::ToggleSelect)
        );
        assert_eq!(normal_action(key(KeyCode::Char('c'))), Some(Action::Copy));
        assert_eq!(normal_action(key(KeyCode::Char('m'))), Some(Action::Move));
        assert_eq!(normal_action(key(KeyCode::Char('r'))), Some(Action::Rename));
        assert_eq!(normal_action(key(KeyCode::Char('d'))), Some(Action::Delete));
        assert_eq!(
            normal_action(key(KeyCode::Char('/'))),
            Some(Action::FilterStart)
        );
        assert_eq!(normal_action(key(KeyCode::Esc)), Some(Action::Escape));
        assert_eq!(
            normal_action(key(KeyCode::Char('.'))),
            Some(Action::ToggleHidden)
        );
        assert_eq!(normal_action(key(KeyCode::Char('q'))), Some(Action::Quit));
        assert_eq!(
            normal_action(key(KeyCode::Char('s'))),
            Some(Action::Settings)
        );
        assert_eq!(
            normal_action(key(KeyCode::Char('b'))),
            Some(Action::BookmarkAdd)
        );
        assert_eq!(
            normal_action(key(KeyCode::Char('B'))),
            Some(Action::BookmarksOpen)
        );
    }

    #[test]
    fn modified_keys_are_ignored_and_ctrl_c_interrupts() {
        let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(normal_action(ctrl_d), None);
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(is_interrupt(&ctrl_c));
        assert!(!is_interrupt(&key(KeyCode::Char('c'))));
    }

    #[test]
    fn no_key_deletes_without_going_through_the_app_dialog() {
        // The only delete bindings produce Action::Delete, which the app turns
        // into a confirmation prompt (tested in app.rs).
        for code in [KeyCode::Char('d'), KeyCode::Char('D'), KeyCode::Delete] {
            assert_eq!(normal_action(key(code)), Some(Action::Delete));
        }
    }
}
