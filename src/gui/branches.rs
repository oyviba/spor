//! The branch list: local branches, each remote's branches, tags and
//! stashes, with a search field on top. Click shows a branch's tip in
//! History; double-click checks out.

use super::app::*;
use super::assets::icon;
use gpui_kit::component::{
    button::ButtonVariants as _,
    h_flex,
    input::Input,
    menu::{ContextMenuExt as _, PopupMenuItem},
    v_flex, ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _,
};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, AnyElement, App, ClickEvent, Context, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use std::rc::Rc;

#[derive(Clone)]
struct Item {
    /// Ref key used to find the tip row (`main`, `origin/main`, `tag:v1`).
    key: String,
    label: String,
    icon: &'static str,
    current: bool,
    trailing: String,
    kind: Kind,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Local,
    Remote,
    Tag,
    Stash(usize),
    /// A linked working tree; the item's key is its path.
    Worktree,
}

impl Spor {
    pub(crate) fn render_branches(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(repo) = &self.repo else {
            return div().into_any_element();
        };
        let selected_hash = self
            .selected
            .and_then(|i| repo.rows.get(i))
            .map(|r| r.commit.hash.clone());
        let tip_selected = |key: &str| {
            selected_hash.as_ref().is_some_and(|h| {
                repo.ref_rows
                    .get(key)
                    .and_then(|&i| repo.rows.get(i))
                    .is_some_and(|r| &r.commit.hash == h)
            }) && self.tab == Tab::History
        };

        let mut sections: Vec<(String, Vec<Item>)> = Vec::new();
        let mut local = Vec::new();
        let mut remotes: Vec<(String, Vec<Item>)> = repo
            .remotes
            .iter()
            .map(|r| (r.clone(), Vec::new()))
            .collect();
        for b in &repo.branches {
            if b.is_remote {
                let (rem, rest) = b.name.split_once('/').unwrap_or(("", &b.name));
                let item = Item {
                    key: b.name.clone(),
                    label: rest.to_string(),
                    icon: icon::GIT_BRANCH,
                    current: false,
                    trailing: String::new(),
                    kind: Kind::Remote,
                };
                match remotes.iter_mut().find(|(r, _)| r == rem) {
                    Some((_, list)) => list.push(item),
                    None => remotes.push((rem.to_string(), vec![item])),
                }
            } else {
                let mut trailing = Vec::new();
                if let Some(pr) = self.prs.get(&b.name) {
                    trailing.push(format!("#{}", pr.number));
                }
                if b.is_current {
                    let t = &repo.tracking;
                    if t.ahead > 0 {
                        trailing.push(format!("↑{}", t.ahead));
                    }
                    if t.behind > 0 {
                        trailing.push(format!("↓{}", t.behind));
                    }
                }
                local.push(Item {
                    key: b.name.clone(),
                    label: b.name.clone(),
                    icon: icon::GIT_BRANCH,
                    current: b.is_current,
                    trailing: trailing.join(" "),
                    kind: Kind::Local,
                });
            }
        }
        sections.push(("Local".into(), local));
        for (name, items) in remotes {
            if !items.is_empty() {
                sections.push((format!("Remote · {name}"), items));
            }
        }
        let mut tags: Vec<(usize, String)> = repo
            .ref_rows
            .iter()
            .filter_map(|(k, &i)| k.strip_prefix("tag:").map(|t| (i, t.to_string())))
            .collect();
        tags.sort();
        if !tags.is_empty() {
            sections.push((
                "Tags".into(),
                tags.into_iter()
                    .map(|(_, t)| Item {
                        key: format!("tag:{t}"),
                        label: t,
                        icon: icon::TAG,
                        current: false,
                        trailing: String::new(),
                        kind: Kind::Tag,
                    })
                    .collect(),
            ));
        }
        if !repo.stashes.is_empty() {
            sections.push((
                "Stashes".into(),
                repo.stashes
                    .iter()
                    .enumerate()
                    .map(|(i, msg)| {
                        let label = stash_label(msg);
                        Item {
                            key: format!("stash@{{{i}}}"),
                            label: label.to_string(),
                            icon: icon::ARCHIVE,
                            current: false,
                            trailing: format!("stash@{{{i}}}"),
                            kind: Kind::Stash(i),
                        }
                    })
                    .collect(),
            ));
        }

        if !repo.worktrees.is_empty() {
            sections.push((
                "Worktrees".into(),
                repo.worktrees
                    .iter()
                    .map(|w| Item {
                        key: w.path.to_string_lossy().into_owned(),
                        label: w.branch.clone().unwrap_or_else(|| {
                            format!("detached {}", &w.head[..w.head.len().min(7)])
                        }),
                        icon: icon::FOLDER_GIT,
                        current: false,
                        trailing: w
                            .path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        kind: Kind::Worktree,
                    })
                    .collect(),
            ));
        }

        let query = self.branch_filter.read(cx).value().trim().to_lowercase();
        let filtering = !query.is_empty();
        if filtering {
            for (_, items) in &mut sections {
                items.retain(|item| matches(item, &query));
            }
            sections.retain(|(_, items)| !items.is_empty());
        }

        let spor = Self::weak(cx);
        let remote_names: Rc<[String]> = repo.remotes.iter().cloned().collect();
        let current: Rc<Option<String>> = Rc::new(repo.tracking.branch.clone());
        let mut list = v_flex().py_1();
        if filtering && sections.is_empty() {
            list = list.child(
                div()
                    .px_3()
                    .py_2()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("No matching branches or tags"),
            );
        }
        for (title, items) in sections {
            list = list.child(
                h_flex()
                    .h(px(26.))
                    .px_3()
                    .mt_1()
                    .text_xs()
                    .font_medium()
                    .text_color(theme.muted_foreground)
                    .child(title)
                    .child(div().flex_1())
                    .child(div().opacity(0.7).child(items.len().to_string())),
            );
            for item in items {
                let selected = tip_selected(&item.key);
                list = list.child(render_item(
                    item,
                    selected,
                    remote_names.clone(),
                    current.clone(),
                    spor.clone(),
                    cx,
                ));
            }
        }

        v_flex()
            .size_full()
            .bg(theme.sidebar)
            .child(
                h_flex()
                    .h(px(44.))
                    .flex_none()
                    .px_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().text_sm().font_semibold().child("Branches"))
                    .child(div().flex_1())
                    .child(
                        gpui_kit::component::button::Button::new("new-branch")
                            .ghost()
                            .small()
                            .icon(IconName::Plus)
                            .tooltip("New Branch… (⌘⇧N)")
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.new_branch(None, window, cx)
                                }),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .px_2()
                    .py_1p5()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        Input::new(&self.branch_filter)
                            .small()
                            .cleanable(true)
                            .prefix(
                                Icon::new(IconName::Search)
                                    .small()
                                    .text_color(theme.muted_foreground),
                            ),
                    ),
            )
            .child(
                div()
                    .id("branch-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .into_any_element()
    }
}

/// "WIP on main: abc123 subject" → "subject"; a stash saved with a
/// message, "On main: message" → "message".
fn stash_label(msg: &str) -> &str {
    match msg.split_once(": ") {
        Some((head, rest)) if head.starts_with("WIP on ") => {
            rest.split_once(' ').map_or(rest, |(_, subject)| subject)
        }
        Some((_, rest)) => rest,
        None => msg,
    }
}

/// Every word of the search appears in the item's name — `origin/fix`
/// finds `fix-login` on origin, `login fix` finds `fix/login`.
fn matches(item: &Item, query: &str) -> bool {
    let haystack = format!("{} {}", item.key, item.label).to_lowercase();
    query.split_whitespace().all(|word| haystack.contains(word))
}

fn render_item(
    item: Item,
    selected: bool,
    remotes: Rc<[String]>,
    current: Rc<Option<String>>,
    spor: gpui_kit::WeakEntity<Spor>,
    cx: &mut App,
) -> AnyElement {
    let theme = cx.theme().clone();
    let icon_color: Hsla = if item.current {
        theme.primary
    } else {
        theme.muted_foreground
    };
    let click = {
        let spor = spor.clone();
        let item = item.clone();
        move |event: &ClickEvent, window: &mut Window, cx: &mut App| {
            let item = item.clone();
            let _ = spor.update(cx, |this, cx| match item.kind {
                Kind::Stash(i) if event.click_count() >= 2 => this.apply_stash(i, window, cx),
                Kind::Stash(_) => {}
                Kind::Worktree if event.click_count() >= 2 => {
                    this.open_repo(std::path::Path::new(&item.key), window, cx)
                }
                Kind::Worktree => {}
                Kind::Local | Kind::Remote if event.click_count() >= 2 && !item.current => {
                    this.checkout(&item.key, window, cx)
                }
                _ => this.reveal_ref(&item.key, window, cx),
            });
        }
    };
    let menu_item = item.clone();
    h_flex()
        .id(SharedString::from(format!("branch-{}", item.key)))
        .h(px(26.))
        .mx_1()
        .px_2()
        .gap_2()
        .rounded(theme.radius)
        .text_sm()
        .when(selected, |this| this.bg(theme.list_active))
        .when(!selected, |this| {
            this.hover(|this| this.bg(theme.list_hover))
        })
        .on_click(click)
        .child(Icon::empty().path(item.icon).small().text_color(icon_color))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .when(item.current, |this| this.font_medium())
                .child(item.label.clone()),
        )
        .when(!item.trailing.is_empty(), |this| {
            this.child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(item.trailing.clone()),
            )
        })
        .when(item.current, |this| {
            this.child(
                Icon::new(IconName::Check)
                    .xsmall()
                    .text_color(theme.primary),
            )
        })
        .context_menu(move |menu, _, _| {
            let act = |label: String, f: SporAction| {
                let spor = spor.clone();
                PopupMenuItem::new(label).on_click(move |_, window, cx| {
                    let f = f.clone();
                    let _ = spor.update(cx, |this, cx| f(this, window, cx));
                })
            };
            let item = menu_item.clone();
            match item.kind {
                Kind::Stash(i) => menu
                    .item(act(
                        "Apply and Remove".into(),
                        Rc::new(move |this, w, cx| this.apply_stash(i, w, cx)),
                    ))
                    .item(act(
                        "Apply and Keep".into(),
                        Rc::new(move |this, w, cx| this.apply_stash_keep(i, w, cx)),
                    ))
                    .separator()
                    .item(act(
                        "Delete Stash".into(),
                        Rc::new(move |this, w, cx| this.drop_stash(i, w, cx)),
                    )),
                Kind::Worktree => {
                    let (open, remove, copy) =
                        (item.key.clone(), item.key.clone(), item.key.clone());
                    menu.item(act(
                        "Open".into(),
                        Rc::new(move |this, w, cx| {
                            this.open_repo(std::path::Path::new(&open), w, cx)
                        }),
                    ))
                    .item(act(
                        "Copy Path".into(),
                        Rc::new(move |this, w, cx| this.copy(copy.clone(), "Path", w, cx)),
                    ))
                    .separator()
                    .item(act(
                        "Remove Working Tree…".into(),
                        Rc::new(move |this, w, cx| {
                            this.remove_worktree(remove.clone().into(), w, cx)
                        }),
                    ))
                }
                kind => {
                    let mut menu = menu;
                    let branch = matches!(kind, Kind::Local | Kind::Remote);
                    if branch && !item.current {
                        let key = item.key.clone();
                        menu = menu.item(act(
                            format!("Check Out {}", item.label),
                            Rc::new(move |this, w, cx| this.checkout(&key, w, cx)),
                        ));
                        if let Some(current) = current.as_ref() {
                            let (merge, rebase) = (item.key.clone(), item.key.clone());
                            menu = menu
                                .item(act(
                                    format!("Merge {} into {current}", item.label),
                                    Rc::new(move |this, w, cx| {
                                        this.merge_into_current(&merge, w, cx)
                                    }),
                                ))
                                .item(act(
                                    format!("Rebase {current} onto {}", item.label),
                                    Rc::new(move |this, w, cx| this.rebase_onto(&rebase, w, cx)),
                                ));
                        }
                    }
                    let key = item.key.clone();
                    let reveal = item.key.clone();
                    let label = item.label.clone();
                    menu.item(act(
                        "Show in History".into(),
                        Rc::new(move |this, w, cx| this.reveal_ref(&reveal, w, cx)),
                    ))
                    .item(act(
                        "New Branch from Here…".into(),
                        Rc::new(move |this, w, cx| {
                            let sha = this
                                .repo
                                .as_ref()
                                .and_then(|r| r.ref_rows.get(&key).and_then(|&i| r.rows.get(i)))
                                .map(|row| row.commit.hash.clone());
                            if sha.is_some() {
                                this.new_branch(sha, w, cx);
                            }
                        }),
                    ))
                    .separator()
                    .item(act(
                        "Copy Name".into(),
                        Rc::new(move |this, w, cx| this.copy(label.clone(), "Name", w, cx)),
                    ))
                    .when(kind == Kind::Local, |menu| {
                        let (rename, delete) = (item.key.clone(), item.key.clone());
                        let menu = menu.separator().item(act(
                            "Rename…".into(),
                            Rc::new(move |this, w, cx| this.rename_branch(&rename, w, cx)),
                        ));
                        if item.current {
                            menu
                        } else {
                            menu.item(act(
                                "Delete Branch".into(),
                                Rc::new(move |this, w, cx| this.delete_branch(&delete, w, cx)),
                            ))
                        }
                    })
                    .when(kind == Kind::Remote, |menu| {
                        let full = item.key.clone();
                        menu.separator().item(act(
                            "Delete from Remote…".into(),
                            Rc::new(move |this, w, cx| this.delete_remote_branch(&full, w, cx)),
                        ))
                    })
                    .when(kind == Kind::Tag, |menu| {
                        let name = item.label.clone();
                        let mut menu = menu.separator().item(act(
                            "Delete Tag".into(),
                            Rc::new({
                                let name = name.clone();
                                move |this, w, cx| this.delete_tag(&name, w, cx)
                            }),
                        ));
                        for remote in remotes.iter() {
                            let (name, remote) = (name.clone(), remote.clone());
                            menu = menu.item(act(
                                format!("Delete Tag from {remote}…"),
                                Rc::new(move |this, w, cx| {
                                    this.delete_remote_tag(&name, &remote, w, cx)
                                }),
                            ));
                        }
                        menu
                    })
                }
            }
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    #[test]
    fn stash_labels() {
        assert_eq!(super::stash_label("WIP on main: abc123 Fix it"), "Fix it");
        assert_eq!(
            super::stash_label("On main: half-done edits"),
            "half-done edits"
        );
        assert_eq!(super::stash_label("odd"), "odd");
    }
}
