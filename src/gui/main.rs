//! spor as a native desktop app, built on GPUI (Zed's UI framework) with the
//! GPUI Kit components. Same core as the terminal UI (`spor::git`,
//! `spor::graph`, …). Built with `--features gui`; on macOS
//! `scripts/bundle-macos.sh` wraps it into `Spor.app`.
//!
//! The layout follows Zed: title bar (repository / branch), the workspace's
//! repositories on the left, a dock with the git panel or the branch list
//! or pull requests or Actions runs (on either side), a center pane with
//! Uncommitted Changes and History tabs (and the PR or run you opened), and
//! a status bar that toggles the panels.

mod app;
mod assets;
mod branches;
mod changes_view;
mod git_panel;
mod graph;
mod history;
mod history_options;
mod hub;
mod keymap;
mod palette;
mod pulls;
mod rail;
mod render;
mod repo;
mod repos_panel;
mod runs;
mod support;
mod theme;
mod theme_editor;
mod welcome;
mod workspaces;

use gpui_kit::component::TitleBar;
use gpui_kit::{px, size, AppContext as _, WindowBounds, WindowOptions};

fn main() {
    support::fix_path_for_gui_launch();
    let initial = support::initial_repo();

    gpui_kit::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            theme::init(cx);
            app::init(cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1360.), px(860.)), cx)),
                window_min_size: Some(size(px(960.), px(520.))),
                app_id: Some("io.github.oyviba.spor".into()),
                ..TitleBar::window_options()
            };
            let initial = initial.clone();
            gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| app::Spor::new(initial.clone(), window, cx))
            })
            .expect("failed to open the Spor window");
            cx.activate(true);
        });
}
