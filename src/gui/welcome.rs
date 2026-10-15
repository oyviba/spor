//! The start page, like Zed's welcome screen: the app's name, an action list
//! with shortcuts, and the active workspace's repositories.

use super::app::*;
use super::assets::icon;
use super::support::tildify;
use gpui_kit::component::{
    h_flex, kbd::Kbd, v_flex, ActiveTheme as _, Icon, IconName, Sizable as _, StyledExt as _,
};
use gpui_kit::{
    div, img, prelude::FluentBuilder as _, px, AnyElement, Context, InteractiveElement as _,
    IntoElement, ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window,
};

impl Spor {
    pub(crate) fn render_welcome(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let open_key = Kbd::binding_for_action(&OpenRepository, Some("Spor"), window);

        let action = h_flex()
            .id("welcome-open")
            .w_full()
            .px_3()
            .py_2()
            .gap_3()
            .rounded(theme.radius)
            .hover(|this| this.bg(theme.list_hover))
            .cursor_pointer()
            .child(
                Icon::new(IconName::FolderOpen)
                    .small()
                    .text_color(theme.muted_foreground),
            )
            .child(div().flex_1().text_sm().child("Open Repository…"))
            .when_some(open_key, |this, key| this.child(key))
            .on_click(cx.listener(|this, _, window, cx| this.pick_repo(window, cx)));
        let theme_key = Kbd::binding_for_action(&EditTheme, Some("Spor"), window);
        let theme_action = h_flex()
            .id("welcome-theme")
            .w_full()
            .px_3()
            .py_2()
            .gap_3()
            .rounded(theme.radius)
            .hover(|this| this.bg(theme.list_hover))
            .cursor_pointer()
            .child(
                Icon::new(IconName::Palette)
                    .small()
                    .text_color(theme.muted_foreground),
            )
            .child(div().flex_1().text_sm().child("Edit Theme…"))
            .when_some(theme_key, |this, key| this.child(key))
            .on_click(cx.listener(|this, _, window, cx| this.open_theme_editor(window, cx)));

        let workspace = self.workspaces.current();
        let workspace_name = workspace.name.clone();
        let recent: Vec<_> = workspace.repos.clone();
        let empty = recent.is_empty();
        let recent_list = v_flex()
            .gap_0p5()
            .children(recent.into_iter().enumerate().map(|(i, path)| {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.display().to_string());
                let location = tildify(path.parent().unwrap_or(&path));
                h_flex()
                    .id(("recent", i))
                    .w_full()
                    .px_3()
                    .py_1p5()
                    .gap_3()
                    .rounded(theme.radius)
                    .hover(|this| this.bg(theme.list_hover))
                    .cursor_pointer()
                    .child(
                        Icon::empty()
                            .path(icon::FOLDER_GIT)
                            .small()
                            .text_color(theme.muted_foreground),
                    )
                    .child(div().text_sm().child(name))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(location),
                    )
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.open_repo(&path, window, cx)),
                    )
            }));

        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.background)
            .child(
                v_flex()
                    .w(px(440.))
                    .gap_6()
                    .child(
                        h_flex()
                            .gap_4()
                            .child(img(SharedString::from("images/spor.png")).size(px(64.)))
                            .child(
                                v_flex()
                                    .child(div().text_2xl().font_semibold().child("Spor"))
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(theme.muted_foreground)
                                            .child("Follow the track of every branch"),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(section_title("Get Started", &theme))
                            .child(action)
                            .child(theme_action)
                            .child(
                                div()
                                    .px_3()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child("You can also drop a folder onto this window."),
                            ),
                    )
                    .when_some(self.open_error.clone(), |this, error| {
                        this.child(
                            h_flex()
                                .px_3()
                                .gap_2()
                                .text_sm()
                                .text_color(theme.danger)
                                .child(Icon::new(IconName::CircleX).small())
                                .child(error),
                        )
                    })
                    .child(
                        v_flex()
                            .gap_1()
                            .child(section_title(workspace_name.clone(), &theme))
                            .when(empty, |this| {
                                this.child(
                                    div()
                                        .px_3()
                                        .text_sm()
                                        .text_color(theme.muted_foreground)
                                        .child(format!(
                                            "No repositories in {workspace_name} yet. Open one to add it."
                                        )),
                                )
                            })
                            .child(recent_list),
                    ),
            )
            .into_any_element()
    }
}

fn section_title(
    title: impl Into<SharedString>,
    theme: &gpui_kit::component::Theme,
) -> impl IntoElement {
    div()
        .px_3()
        .pb_1()
        .text_xs()
        .font_medium()
        .text_color(theme.muted_foreground)
        .child(title.into())
}
