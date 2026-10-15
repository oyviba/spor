//! User-assignable shortcuts. Every command in `palette::SHORTCUTS` has
//! default keys; the user can record others in the ⌘/ sheet, remove them or
//! reset them. Changes are saved as overrides (`keymap` in the config
//! directory, one `action<TAB>key<TAB>key…` line per changed command) and
//! take effect immediately.

use super::app::*;
use super::palette::{render_keys, Shortcut, SHORTCUTS};
use super::support;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{ContextMenuExt as _, PopupMenuItem},
    v_flex, ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _, WindowExt as _,
};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, AnyElement, App, AppContext as _, Context,
    DummyKeyboardMapper, Global, InteractiveElement as _, IntoElement, KeyBinding,
    KeyBindingContextPredicate, Keystroke, ParentElement as _, Render, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window,
};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

const SETTING: &str = "keymap";

/// Keys that are only modifiers; pressing one alone records nothing.
const MODIFIER_KEYS: &[&str] = &["shift", "control", "alt", "platform", "function"];

/// The user's changes to the default shortcuts, by action name. An empty
/// list means the command was deliberately left without a shortcut.
#[derive(Default)]
pub struct UserKeymap {
    overrides: HashMap<String, Vec<String>>,
}

impl Global for UserKeymap {}

/// A key in one spelling, so `secondary-p` and a recorded `cmd-p` compare
/// equal: parsed, then printed back for this platform.
fn canonical(key: &str) -> Option<String> {
    Keystroke::parse(key).ok().map(|k| k.unparse())
}

fn defaults(s: &Shortcut) -> Vec<String> {
    s.default_keys.iter().filter_map(|k| canonical(k)).collect()
}

impl UserKeymap {
    fn saved() -> Self {
        let mut overrides = HashMap::new();
        for line in support::read_setting(SETTING).unwrap_or_default().lines() {
            let mut fields = line.split('\t');
            let Some(name) = fields.next().filter(|n| !n.is_empty()) else {
                continue;
            };
            let keys = fields.filter_map(canonical).collect();
            overrides.insert(name.to_string(), keys);
        }
        Self { overrides }
    }

    fn save(&self) {
        let mut names: Vec<&String> = self.overrides.keys().collect();
        names.sort();
        let body: String = names
            .into_iter()
            .map(|name| {
                let mut line = name.clone();
                for key in &self.overrides[name] {
                    line.push('\t');
                    line.push_str(key);
                }
                line + "\n"
            })
            .collect();
        support::write_setting(SETTING, &body);
    }

    /// The keys `s` has now.
    pub fn keys(&self, s: &Shortcut) -> Vec<String> {
        self.overrides
            .get(s.name())
            .cloned()
            .unwrap_or_else(|| defaults(s))
    }

    /// Has the user changed this one?
    pub fn is_custom(&self, s: &Shortcut) -> bool {
        self.overrides
            .get(s.name())
            .is_some_and(|keys| *keys != defaults(s))
    }

    fn set(&mut self, s: &Shortcut, keys: Vec<String>) {
        if keys == defaults(s) {
            self.overrides.remove(s.name());
        } else {
            self.overrides.insert(s.name().to_string(), keys);
        }
    }

    /// Take `key` away from every other command it would collide with,
    /// returning their labels.
    fn release(&mut self, key: &str, keep: usize) -> Vec<&'static str> {
        let mut taken = Vec::new();
        for (ix, other) in SHORTCUTS.iter().enumerate() {
            if ix == keep || !other.assignable() || other.context != SHORTCUTS[keep].context {
                continue;
            }
            let mut keys = self.keys(other);
            if keys.iter().any(|k| k == key) {
                keys.retain(|k| k != key);
                self.set(other, keys);
                taken.push(other.label);
            }
        }
        taken
    }
}

pub fn init(cx: &mut App) {
    cx.set_global(UserKeymap::saved());
    apply(cx);
}

/// Rebuild the app's bindings for every assignable command from the user's
/// keymap, leaving everyone else's (the components', ⌘1–⌘9) as they are.
fn apply(cx: &mut App) {
    let map = cx.global::<UserKeymap>();
    let ours: HashSet<&'static str> = SHORTCUTS
        .iter()
        .filter(|s| s.assignable())
        .map(|s| s.name())
        .collect();
    let mut mine = Vec::new();
    for s in SHORTCUTS.iter().filter(|s| s.assignable()) {
        for key in map.keys(s) {
            for context in [Some(s.context), s.also_in].into_iter().flatten() {
                let predicate = KeyBindingContextPredicate::parse(context).ok().map(Rc::new);
                if let Ok(binding) = KeyBinding::load(
                    &key,
                    (s.action)(),
                    predicate,
                    false,
                    None,
                    &DummyKeyboardMapper,
                ) {
                    mine.push(binding);
                }
            }
        }
    }
    let others: Vec<KeyBinding> = cx
        .key_bindings()
        .borrow()
        .bindings()
        .filter(|b| !ours.contains(b.action().name()))
        .cloned()
        .collect();
    cx.clear_key_bindings();
    cx.bind_keys(others);
    cx.bind_keys(mine);
}

fn change(cx: &mut App, f: impl FnOnce(&mut UserKeymap) -> Vec<&'static str>) -> Vec<&'static str> {
    let taken = f(cx.global_mut::<UserKeymap>());
    cx.global::<UserKeymap>().save();
    apply(cx);
    taken
}

/// "Moved from Refresh" / "Moved from Refresh and Undo".
fn moved_from(taken: &[&str]) -> Option<String> {
    match taken {
        [] => None,
        [one] => Some(format!("Taken from “{one}”, which now has no shortcut")),
        many => Some(format!(
            "Taken from {}, which now have no shortcut",
            many.iter()
                .map(|l| format!("“{l}”"))
                .collect::<Vec<_>>()
                .join(" and ")
        )),
    }
}

#[derive(Clone)]
struct Note {
    row: Option<usize>,
    error: bool,
    text: String,
}

impl Note {
    fn about(row: usize, error: bool, text: String) -> Self {
        Self {
            row: Some(row),
            error,
            text,
        }
    }
}

/// The ⌘/ sheet: every shortcut, and the place to change them.
pub struct ShortcutsView {
    /// The row waiting for a key.
    recording: Option<usize>,
    /// The last outcome, shown under the row it's about (or at the top).
    message: Option<Note>,
    _intercept: Subscription,
}

impl ShortcutsView {
    fn new(cx: &mut Context<Self>) -> Self {
        let view = cx.entity().downgrade();
        // While recording, every key comes here first — before bindings,
        // so even the one being replaced (or ⌘Q) is recorded, not run.
        let intercept = cx.intercept_keystrokes(move |event, window, cx| {
            let Some(view) = view.upgrade() else { return };
            if view.read(cx).recording.is_none() {
                return;
            }
            cx.stop_propagation();
            let keystroke = event.keystroke.clone();
            view.update(cx, |this, cx| this.record(keystroke, window, cx));
        });
        Self {
            recording: None,
            message: None,
            _intercept: intercept,
        }
    }

    fn start(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.recording = Some(ix);
        self.message = None;
        cx.notify();
    }

    fn record(&mut self, keystroke: Keystroke, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.recording else { return };
        if MODIFIER_KEYS.contains(&keystroke.key.as_str()) {
            return;
        }
        let m = keystroke.modifiers;
        let bare = !(m.platform || m.control || m.alt || m.function || m.shift);
        if bare && keystroke.key == "escape" {
            self.recording = None;
            cx.notify();
            return;
        }
        if bare && matches!(keystroke.key.as_str(), "backspace" | "delete") {
            self.recording = None;
            self.remove(ix, cx);
            return;
        }
        let key = Keystroke {
            modifiers: m,
            key: keystroke.key.clone(),
            key_char: None,
        }
        .unparse();
        match self.check(ix, &key) {
            Err(e) => self.message = Some(Note::about(ix, true, e)),
            Ok(()) => {
                self.recording = None;
                let taken = change(cx, |map| {
                    let taken = map.release(&key, ix);
                    map.set(&SHORTCUTS[ix], vec![key.clone()]);
                    taken
                });
                self.message = moved_from(&taken).map(|m| Note::about(ix, false, m));
            }
        }
        cx.notify();
    }

    /// Would `key` make a usable shortcut for row `ix`?
    fn check(&self, ix: usize, key: &str) -> Result<(), String> {
        let s = &SHORTCUTS[ix];
        let stroke = Keystroke::parse(key).map_err(|e| e.to_string())?;
        let m = stroke.modifiers;
        let function_key = stroke.key.len() > 1
            && stroke.key.starts_with('f')
            && stroke.key[1..].chars().all(|c| c.is_ascii_digit());
        // Outside History the keys also reach text fields, where a bare
        // letter (or shift + letter) has to type itself.
        if s.context == "Spor" && !(m.platform || m.control || m.alt || m.function || function_key)
        {
            return Err("Add ⌘, ⌃ or ⌥ — a key on its own would get in the way of typing".into());
        }
        let workspace_keys = (1..=9).filter_map(|n| canonical(&format!("secondary-{n}")));
        if s.context == "Spor" && workspace_keys.into_iter().any(|k| k == key) {
            return Err("That one switches workspaces — pick another".into());
        }
        Ok(())
    }

    fn remove(&mut self, ix: usize, cx: &mut Context<Self>) {
        change(cx, |map| {
            map.set(&SHORTCUTS[ix], Vec::new());
            Vec::new()
        });
        self.message = None;
        cx.notify();
    }

    fn reset(&mut self, ix: usize, cx: &mut Context<Self>) {
        // A default coming back takes its key from whoever has it now.
        let taken = change(cx, |map| {
            let s = &SHORTCUTS[ix];
            let mut taken = Vec::new();
            for key in defaults(s) {
                taken.extend(map.release(&key, ix));
            }
            map.overrides.remove(s.name());
            taken
        });
        self.message = moved_from(&taken).map(|m| Note::about(ix, false, m));
        cx.notify();
    }

    fn reset_all(&mut self, cx: &mut Context<Self>) {
        change(cx, |map| {
            map.overrides.clear();
            Vec::new()
        });
        self.recording = None;
        self.message = Some(Note {
            row: None,
            error: false,
            text: "Every shortcut is back to its default".into(),
        });
        cx.notify();
    }

    fn render_row(
        &self,
        ix: usize,
        custom: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let s = &SHORTCUTS[ix];
        let recording = self.recording == Some(ix);
        let assignable = s.assignable();
        let has_keys = !render_keys_empty(s, window);
        let view = cx.entity().downgrade();
        let keys = if recording {
            div()
                .px_2()
                .rounded(theme.radius)
                .border_1()
                .border_color(theme.primary)
                .text_xs()
                .text_color(theme.primary)
                .child("Press a shortcut…")
                .into_any_element()
        } else if has_keys {
            render_keys(s, window, cx)
        } else {
            div()
                .text_xs()
                .text_color(theme.muted_foreground.opacity(0.7))
                .child("None")
                .into_any_element()
        };
        let group: SharedString = format!("shortcut-row-{ix}").into();
        let row = h_flex()
            .id(("shortcut", ix))
            .h(px(30.))
            .px_2()
            .gap_2()
            .rounded(theme.radius)
            .text_sm()
            .group(group.clone())
            .when(recording, |this| this.bg(theme.list_active))
            .when(assignable && !recording, |this| {
                this.cursor_pointer()
                    .hover(|this| this.bg(theme.list_hover))
            })
            .when(assignable, |this| {
                this.on_click(cx.listener(move |this, _, _, cx| this.start(ix, cx)))
            })
            .child(div().flex_1().min_w_0().truncate().child(s.label))
            .when(custom && !recording, |this| {
                this.child(
                    div()
                        .flex_none()
                        .opacity(0.)
                        .group_hover(group, |this| this.opacity(1.))
                        .child(
                            Button::new(("reset-shortcut", ix))
                                .ghost()
                                .xsmall()
                                .label("Reset")
                                .tooltip("Back to the default shortcut")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.reset(ix, cx)
                                })),
                        ),
                )
            })
            .when(custom && !recording, |this| {
                this.child(
                    div()
                        .flex_none()
                        .size(px(6.))
                        .rounded_full()
                        .bg(theme.primary),
                )
            })
            .child(keys);
        if !assignable {
            return row.into_any_element();
        }
        row.context_menu(move |menu, _, _| {
            let item =
                |label: &'static str,
                 f: fn(&mut ShortcutsView, usize, &mut Context<ShortcutsView>)| {
                    let view = view.clone();
                    PopupMenuItem::new(label).on_click(move |_, _, cx| {
                        let _ = view.update(cx, |this, cx| f(this, ix, cx));
                    })
                };
            menu.item(item("Change Shortcut…", |this, ix, cx| {
                this.start(ix, cx)
            }))
            .item(item("Remove Shortcut", |this, ix, cx| this.remove(ix, cx)))
            .item(item("Reset to Default", |this, ix, cx| this.reset(ix, cx)))
        })
        .into_any_element()
    }
}

/// Does `s` have no key bound right now?
fn render_keys_empty(s: &Shortcut, window: &Window) -> bool {
    super::palette::keys(&*(s.action)(), s.context, window).is_empty()
}

impl Render for ShortcutsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let map = cx.global::<UserKeymap>();
        let custom: Vec<bool> = SHORTCUTS.iter().map(|s| map.is_custom(s)).collect();
        let any_custom = custom.iter().any(|c| *c);

        let mut groups: Vec<&'static str> = Vec::new();
        for s in SHORTCUTS {
            if !groups.contains(&s.group) {
                groups.push(s.group);
            }
        }
        let mut list = v_flex().gap_4().pb_4();
        for group in groups {
            let rows: Vec<_> = SHORTCUTS
                .iter()
                .enumerate()
                .filter(|(_, s)| s.group == group)
                .map(|(ix, _)| {
                    let note = self.message.clone().filter(|n| n.row == Some(ix));
                    v_flex()
                        .child(self.render_row(ix, custom[ix], window, cx))
                        .when_some(note, |this, note| this.child(render_note(&note, cx)))
                        .into_any_element()
                })
                .collect();
            list = list.child(
                v_flex()
                    .child(
                        div()
                            .px_2()
                            .pb_1()
                            .mb_1()
                            .border_b_1()
                            .border_color(theme.border)
                            .text_xs()
                            .font_medium()
                            .text_color(theme.muted_foreground)
                            .child(group),
                    )
                    .children(rows),
            );
        }

        let hint = if self.recording.is_some() {
            "Press the new shortcut. Esc cancels, ⌫ removes it."
        } else {
            "Click a shortcut to change it; right-click for more."
        };
        let note = self.message.clone().filter(|n| n.row.is_none());
        let header = v_flex()
            .gap_1()
            .pb_3()
            .child(
                h_flex()
                    .min_h(px(24.))
                    .gap_2()
                    .child(
                        h_flex()
                            .flex_1()
                            .gap_1()
                            .px_2()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(Icon::new(IconName::Info).xsmall())
                            .child(hint),
                    )
                    .when(any_custom, |this| {
                        this.child(
                            Button::new("reset-all-shortcuts")
                                .ghost()
                                .xsmall()
                                .label("Reset All")
                                .on_click(cx.listener(|this, _, _, cx| this.reset_all(cx))),
                        )
                    }),
            )
            .when_some(note, |this, note| this.child(render_note(&note, cx)));

        // The sheet scrolls its body, so this is just the content.
        v_flex().child(header).child(list)
    }
}

fn render_note(note: &Note, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .px_2()
        .pb_1()
        .text_xs()
        .text_color(if note.error {
            theme.red
        } else {
            theme.muted_foreground
        })
        .child(note.text.clone())
}

impl Spor {
    /// ⌘/: every keyboard shortcut, grouped, in a sheet beside the window,
    /// where each can be changed.
    pub(crate) fn open_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.new(ShortcutsView::new);
        window.open_sheet(cx, move |sheet, _, _| {
            sheet
                .title("Keyboard Shortcuts")
                .size(px(400.))
                .overlay(false)
                .child(view.clone())
        });
    }
}
