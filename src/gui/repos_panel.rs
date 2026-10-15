//! The repository list, beside the workspace rail: every repository in the
//! active workspace, the open one highlighted with its branch. Click to open
//! one; right-click to reveal or remove it. ⌘⇧E hides the panel.

use super::app::*;
use super::assets::icon;
use super::support::tildify;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{ContextMenuExt as _, PopupMenuItem},
    v_flex, ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _,
};
use gpui_kit::{
    div, prelude::FluentBuilder as _, px, AnyElement, Context, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window,
};
use std::path::PathBuf;

pub const REPOS_WIDTH: f32 = 220.;

impl Spor {
    pub(crate) fn render_repos_panel(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let spor = Self::weak(cx);
        let workspace = self.workspaces.current();
        let open_root = self.repo.as_ref().map(|r| r.root.clone());
        let branch = self.repo.as_ref().map(|r| {
            r.tracking
                .branch
                .clone()
                .unwrap_or_else(|| "detached".into())
        });
        let changes = self.repo.as_ref().map_or(0, |r| r.changes.len());

        let header = h_flex()
            .h(px(36.))
            .flex_none()
            .pl_3()
            .pr_1()
            .gap_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_medium()
                    .child(workspace.name.clone()),
            )
            .child(
                Button::new("add-repository")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Plus)
                    .tooltip("Add Repository… (⌘O)")
                    .on_click(cx.listener(|this, _, window, cx| this.pick_repo(window, cx))),
            );

        let repos: Vec<PathBuf> = workspace.repos.clone();
        let list = if repos.is_empty() {
            v_flex()
                .flex_1()
                .px_3()
                .py_4()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("No repositories"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground.opacity(0.7))
                        .child("Open one to add it to this workspace"),
                )
                .into_any_element()
        } else {
            let rows = repos.into_iter().enumerate().map(|(ix, path)| {
                let current = open_root.as_ref() == Some(&path);
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                // The open repository shows its branch; the others where
                // they live, since several can share a name.
                let detail = match (&branch, current) {
                    (Some(branch), true) => branch.clone(),
                    _ => tildify(path.parent().unwrap_or(&path)),
                };
                let tip: SharedString = tildify(&path).into();
                let open_path = path.clone();
                let menu_path = path.clone();
                let menu_spor = spor.clone();
                h_flex()
                    .id(("repository", ix))
                    .mx_1()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .rounded(theme.radius)
                    .cursor_pointer()
                    .when(current, |this| this.bg(theme.list_active))
                    .when(!current, |this| {
                        this.hover(|this| this.bg(theme.list_hover))
                    })
                    .child(
                        Icon::empty()
                            .path(icon::FOLDER_GIT)
                            .small()
                            .text_color(if current {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            }),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_sm()
                                    .when(current, |this| this.font_medium())
                                    .child(name),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .when(current && branch.is_some(), |this| {
                                        this.child(Icon::empty().path(icon::GIT_BRANCH).xsmall())
                                    })
                                    .child(div().min_w_0().truncate().child(detail)),
                            ),
                    )
                    .when(current && changes > 0, |this| {
                        this.child(
                            div()
                                .flex_none()
                                .text_xs()
                                .text_color(theme.yellow)
                                .child(changes.to_string()),
                        )
                    })
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if this.repo.as_ref().map(|r| &r.root) != Some(&open_path) {
                            this.open_repo(&open_path, window, cx);
                        }
                    }))
                    .context_menu(move |menu, _, _| {
                        let reveal = menu_path.clone();
                        let remove_path = menu_path.clone();
                        let remove = menu_spor.clone();
                        menu.item(
                            PopupMenuItem::new(if cfg!(target_os = "macos") {
                                "Reveal in Finder"
                            } else {
                                "Reveal in File Manager"
                            })
                            .on_click(move |_, _, _| reveal_dir(&reveal)),
                        )
                        .separator()
                        .item(
                            PopupMenuItem::new("Remove from Workspace").on_click(
                                move |_, window, cx| {
                                    let _ = remove.update(cx, |this, cx| {
                                        this.remove_repo(&remove_path, window, cx)
                                    });
                                },
                            ),
                        )
                    })
            });
            div()
                .id("repository-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(v_flex().py_1().gap_0p5().children(rows))
                .into_any_element()
        };

        v_flex()
            .flex_none()
            .w(px(REPOS_WIDTH))
            .h_full()
            .bg(theme.sidebar)
            .border_r_1()
            .border_color(theme.border)
            .child(header)
            .child(list)
            .into_any_element()
    }
}

/// Show a repository's folder in Finder / the file manager.
fn reveal_dir(path: &std::path::Path) {
    let _ = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(path).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(path).spawn()
    };
}
