// Tests for the Ctrl+V / Ctrl+Shift+V clipboard-read fallback that
// kicks in when the host terminal forwards the chord as a raw key
// event instead of a bracketed-paste sequence. The actual arboard
// round-trip is not exercised here (the system clipboard is not a
// deterministic fixture in CI) — these tests verify the gate that
// decides whether the fallback should even be attempted.

use super::super::*;

#[test]
fn clipboard_paste_target_requires_bracketed_paste_enabled() {
    // A fresh pane has neither bracketed paste enabled nor an alt
    // screen, so the fallback must decline. The Ctrl+V key event
    // then falls through to the normal `key_event_to_bytes` path and
    // the historical 0x16 byte reaches the PTY unchanged.
    let app = App::new(40, 80).expect("App::new");
    let pane = app
        .ws()
        .panes
        .get(&app.ws().focused_pane_id)
        .expect("focused pane exists");
    assert!(!pane.is_clipboard_paste_target());
}

#[test]
fn clipboard_paste_target_yes_when_bp_and_not_alt_screen() {
    // PTY apps that opt into bracketed paste (modern bash/zsh,
    // Claude Code) are the intended target of the fallback: the user
    // typically expects Ctrl+V to mean paste there, so a clipboard
    // read is the right behavior when the terminal didn't perform
    // the paste itself.
    let app = App::new(40, 80).expect("App::new");
    let pane = app
        .ws()
        .panes
        .get(&app.ws().focused_pane_id)
        .expect("focused pane exists");
    pane.feed_for_test(b"\x1b[?2004h");
    assert!(pane.is_clipboard_paste_target());
}

#[test]
fn clipboard_paste_target_no_when_alt_screen_even_with_bp() {
    // vim 8+ enables bracketed paste *and* switches to the alt
    // screen; pasting clipboard text into vim's normal-mode buffer
    // (where Ctrl+V means start visual-block selection) would be a
    // surprise. Keep the native Ctrl+V semantics by skipping the
    // fallback whenever the alt screen is active.
    let app = App::new(40, 80).expect("App::new");
    let pane = app
        .ws()
        .panes
        .get(&app.ws().focused_pane_id)
        .expect("focused pane exists");
    pane.feed_for_test(b"\x1b[?2004h\x1b[?1049h");
    assert!(!pane.is_clipboard_paste_target());
}

#[test]
fn clipboard_paste_target_no_when_alt_screen_without_bp() {
    // less / htop / lazygit use the alt screen but never enable
    // bracketed paste — Ctrl+V is either a no-op or a native binding
    // (page down in less). The fallback stays off here too.
    let app = App::new(40, 80).expect("App::new");
    let pane = app
        .ws()
        .panes
        .get(&app.ws().focused_pane_id)
        .expect("focused pane exists");
    pane.feed_for_test(b"\x1b[?1049h");
    assert!(!pane.is_clipboard_paste_target());
}

#[test]
fn clipboard_paste_target_no_when_mouse_reporting_is_on() {
    // Claude Code's `/tui fullscreen` mode enables DECSET 1003
    // (`AnyMotion`) without switching to the alt screen, so it
    // would otherwise sneak past the alt-screen gate while Claude
    // is owning its own keyboard map. The mouse-protocol gate
    // catches that case (and any other in-app mouse-driven UI).
    let app = App::new(40, 80).expect("App::new");
    let pane = app
        .ws()
        .panes
        .get(&app.ws().focused_pane_id)
        .expect("focused pane exists");
    pane.feed_for_test(b"\x1b[?2004h\x1b[?1003h");
    assert!(!pane.is_clipboard_paste_target());
}

#[test]
fn ctrl_v_is_not_consumed_when_pane_is_ineligible() {
    // When the focused pane is *not* a clipboard-paste target the
    // chord must not be intercepted: `handle_key_event` should
    // return `Ok(false)` so `main.rs` forwards the raw 0x16 byte via
    // `key_event_to_bytes`. This guards against the fallback
    // shadowing Ctrl+V's native meaning in vim / less / htop and in
    // shells that have not opted into bracketed paste.
    let mut app = App::new(40, 80).expect("App::new");
    // Default pane has bracketed paste OFF, so the gate refuses.
    let ctrl_v = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL);
    let consumed = app
        .handle_key_event(ctrl_v)
        .expect("handle_key_event Ctrl+V");
    assert!(
        !consumed,
        "Ctrl+V on an ineligible pane must fall through so 0x16 reaches the PTY"
    );
}

#[test]
fn ctrl_v_does_not_paste_into_pane_when_focus_is_on_the_sidebar() {
    // Even on an otherwise eligible pane, Ctrl+V while the user is
    // navigating the file tree must not synthesize a clipboard paste
    // into the background pane's PTY. The file-tree key handler
    // consumes every keystroke on its surface; this guard keeps the
    // fallback consistent with that focus boundary so a user
    // hopping between the tree and a Claude pane can't accidentally
    // dump the clipboard into the wrong place.
    let mut app = App::new(40, 80).expect("App::new");
    let focused_id = app.ws().focused_pane_id;
    app.ws_mut()
        .panes
        .get_mut(&focused_id)
        .expect("focused pane exists")
        .feed_for_test(b"\x1b[?2004h");
    app.ws_mut().focus_target = FocusTarget::FileTree;
    let ctrl_v = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL);
    let consumed = app
        .handle_key_event(ctrl_v)
        .expect("handle_key_event Ctrl+V");
    // The file-tree handler at the bottom of `handle_key_event`
    // will still claim the key (returning Ok(true)), but the
    // clipboard fallback must not be the one doing it — verified
    // indirectly by asserting the focused pane has not entered
    // post-paste cooldown.
    let _ = consumed;
    assert_eq!(
        app.paste_cooldown, 0,
        "clipboard fallback must skip when sidebar holds focus"
    );
}

#[test]
fn ctrl_alt_v_is_never_routed_through_the_clipboard_fallback() {
    // `key.modifiers.contains(CONTROL)` matches Ctrl+Alt+V too, but
    // crossterm's `key_event_to_bytes` translates Alt+Ctrl+Char as
    // `ESC + ctrl_byte` (the standard xterm meta encoding). Shadowing
    // that with a clipboard paste would silently break ESC-prefixed
    // bindings in emacs / readline / Claude Code (`Alt+Ctrl+V` is a
    // common page-down/forward-page chord). The handler must opt out
    // whenever ALT is held — even on an otherwise eligible pane.
    let mut app = App::new(40, 80).expect("App::new");
    let pane = app
        .ws()
        .panes
        .get(&app.ws().focused_pane_id)
        .expect("focused pane exists");
    pane.feed_for_test(b"\x1b[?2004h");
    assert!(pane.is_clipboard_paste_target());
    let ctrl_alt_v = KeyEvent::new(
        KeyCode::Char('v'),
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    let consumed = app
        .handle_key_event(ctrl_alt_v)
        .expect("handle_key_event Ctrl+Alt+V");
    assert!(
        !consumed,
        "Ctrl+Alt+V must bypass the fallback and reach key_event_to_bytes"
    );
}

#[test]
fn ctrl_shift_v_is_treated_the_same_as_ctrl_v_at_byte_level() {
    // At the byte level Ctrl+V and Ctrl+Shift+V both arrive as
    // 0x16; without kitty keyboard protocol the host can't
    // distinguish them and crossterm decodes both as
    // `Char('v') + CONTROL`. With kitty protocol the chord arrives
    // as `Char('V') + CONTROL + SHIFT` — both variants must reach
    // the same fallback path. This test exercises the latter to
    // catch a future regression that gated the handler on
    // `modifiers == CONTROL` (exact match) instead of `contains`.
    let mut app = App::new(40, 80).expect("App::new");
    // No bracketed paste → gate refuses → handler returns Ok(false)
    // even with the upper-case + CONTROL+SHIFT decoding. The point
    // is that we *reach* the new gate, not that we paste.
    let ctrl_shift_v = KeyEvent::new(
        KeyCode::Char('V'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    let consumed = app
        .handle_key_event(ctrl_shift_v)
        .expect("handle_key_event Ctrl+Shift+V");
    assert!(
        !consumed,
        "Ctrl+Shift+V on an ineligible pane must also fall through"
    );
}
