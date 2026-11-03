//! Headless UI tests: the real `Spor` view in a GPUI Kit test window, on a
//! scratch repository, driven by actions and pointer events.
//!
//! Run with `cargo test --features ui-tests`. Settings go to a scratch config
//! directory, and the tests take turns, because opening a repository changes
//! the process's working directory.

use super::app::{DockPanel, Spor, Tab, ToggleRepositories};
use super::history_options::{Column, HistoryOptions};
use super::repos_panel::{REPOS_MIN_WIDTH, REPOS_WIDTH};
use super::{app, support, theme};
use gpui_kit::{
    point, px, size, AnyWindowHandle, AppContext as _, Entity, Modifiers, MouseButton,
    MouseDownEvent, MouseUpEvent, Pixels, Point, TestAppContext, VisualTestContext, WindowBounds,
    WindowOptions,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, Once};

static SERIAL: Mutex<()> = Mutex::new(());

/// Take the test lock, and point the config directory (and git's global
/// config) at a scratch directory the first time.
fn isolate() -> MutexGuard<'static, ()> {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let home = std::env::temp_dir().join(format!("spor-ui-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
    });
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A repository with two commits and one uncommitted change.
fn scratch_repo() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "spor-ui-repo-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    git(&dir, &["config", "user.name", "Spor Test"]);
    git(&dir, &["config", "user.email", "spor@example.com"]);
    std::fs::write(dir.join("README.md"), "# scratch\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "First commit"]);
    std::fs::write(dir.join("notes.txt"), "one\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "Add notes"]);
    std::fs::write(dir.join("notes.txt"), "one\ntwo\n").unwrap();
    dir.canonicalize().unwrap()
}

/// Open the app's window on `repo`, sized like the real one, and draw it.
fn open(cx: &mut TestAppContext, repo: &Path) -> (VisualTestContext, Entity<Spor>) {
    let repo = repo.to_path_buf();
    let (window, spor): (AnyWindowHandle, Entity<Spor>) = cx.update(|cx| {
        gpui_kit::init(cx);
        theme::init(cx);
        app::init(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(gpui_kit::Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(1360.), px(860.)),
            })),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| Spor::new(Some(repo), window, cx))
        })
        .expect("test window opens")
    });
    let mut cx = VisualTestContext::from_window(window, cx);
    cx.run_until_parked();
    redraw(&mut cx);
    (cx, spor)
}

fn redraw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.refresh();
        window.draw(cx).clear(cx);
    });
}

fn bounds_of(cx: &mut VisualTestContext, selector: &'static str) -> gpui_kit::Bounds<Pixels> {
    cx.debug_bounds(selector)
        .unwrap_or_else(|| panic!("nothing drawn as {selector:?}"))
}

/// Press at `from`, move to `to` in steps, release there.
fn drag(cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) {
    let none = Modifiers::none();
    cx.simulate_mouse_move(from, None, none);
    cx.simulate_mouse_down(from, MouseButton::Left, none);
    for step in 1..=4 {
        let t = step as f32 / 4.;
        let at = point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
        cx.simulate_mouse_move(at, MouseButton::Left, none);
        redraw(cx);
    }
    cx.simulate_mouse_up(to, MouseButton::Left, none);
    redraw(cx);
}

fn double_click(cx: &mut VisualTestContext, at: Point<Pixels>) {
    let modifiers = Modifiers::none();
    cx.simulate_mouse_move(at, None, modifiers);
    for click_count in 1..=2 {
        cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers,
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: at,
            modifiers,
            button: MouseButton::Left,
            click_count,
        });
    }
    redraw(cx);
}

#[gpui_kit::test]
fn opens_the_repository_on_its_changes(cx: &mut TestAppContext) {
    let _lock = isolate();
    let repo = scratch_repo();
    let (cx, spor) = open(cx, &repo);

    spor.read_with(&cx, |spor, _| {
        let snapshot = spor.repo.as_ref().expect("repository opened");
        assert_eq!(snapshot.root, repo);
        assert_eq!(snapshot.changes.len(), 1, "notes.txt is modified");
        assert_eq!(snapshot.rows.len(), 2, "two commits in the graph");
        assert_eq!(spor.tab, Tab::Changes);
    });
    // It's remembered for next launch.
    assert_eq!(support::recent_repos().first(), Some(&repo));
}

#[gpui_kit::test]
fn repository_list_resizes_and_hides(cx: &mut TestAppContext) {
    let _lock = isolate();
    let repo = scratch_repo();
    let (mut cx, spor) = open(cx, &repo);
    if !spor.read_with(&cx, |spor, _| spor.layout.show_repos) {
        // An earlier test's saved layout hid it.
        cx.dispatch_action(ToggleRepositories);
        redraw(&mut cx);
    }

    let width = f32::from(bounds_of(&mut cx, "repository-list").size.width);
    assert!(
        (width - REPOS_WIDTH).abs() <= 1.,
        "starts at its default width, got {width}"
    );

    // Its right edge drags, within limits.
    let list = bounds_of(&mut cx, "repository-list");
    let edge = point(list.right(), list.center().y);
    drag(&mut cx, edge, point(edge.x + px(60.), edge.y));
    let wider = f32::from(bounds_of(&mut cx, "repository-list").size.width);
    assert!(
        (wider - REPOS_WIDTH - 60.).abs() <= 2.,
        "dragged 60px wider, got {wider}"
    );
    let list = bounds_of(&mut cx, "repository-list");
    let edge = point(list.right(), list.center().y);
    drag(&mut cx, edge, point(edge.x - px(400.), edge.y));
    let narrowest = f32::from(bounds_of(&mut cx, "repository-list").size.width);
    assert!(
        (narrowest - REPOS_MIN_WIDTH).abs() <= 2.,
        "stops at its minimum, got {narrowest}"
    );

    cx.dispatch_action(ToggleRepositories);
    redraw(&mut cx);
    assert!(cx.debug_bounds("repository-list").is_none());

    // Put it back, for the layout the other tests load.
    cx.dispatch_action(ToggleRepositories);
}

#[gpui_kit::test]
fn history_columns_resize_by_dragging_and_reset_on_double_click(cx: &mut TestAppContext) {
    let _lock = isolate();
    let repo = scratch_repo();
    let (mut cx, spor) = open(cx, &repo);
    spor.update(&mut cx, |spor, cx| {
        spor.set_tab(Tab::History);
        spor.history_options = HistoryOptions::default();
        cx.notify();
    });
    redraw(&mut cx);
    let before = spor.read_with(&cx, |spor, _| spor.history_options.widths);

    // The author column's left edge, dragged 40px left, widens it by 40px.
    let handle = bounds_of(&mut cx, "column-handle-Author").center();
    drag(&mut cx, handle, point(handle.x - px(40.), handle.y));
    let after = spor.read_with(&cx, |spor, _| spor.history_options.widths);
    assert!(
        (after.author - before.author - 40.).abs() <= 1.,
        "author {} → {}",
        before.author,
        after.author
    );
    assert_eq!(after.date, before.date, "other columns stay put");
    // Letting go saves it.
    assert_eq!(HistoryOptions::saved().widths, after);

    // Dragging far past the limit stops at the column's maximum.
    let handle = bounds_of(&mut cx, "column-handle-Branch").center();
    drag(&mut cx, handle, point(handle.x + px(900.), handle.y));
    let branch = spor.read_with(&cx, |spor, _| spor.history_options.widths.branch);
    assert_eq!(branch, Column::Branch.range().1);

    // Double-clicking a handle puts just that column back.
    let handle = bounds_of(&mut cx, "column-handle-Author").center();
    double_click(&mut cx, handle);
    let reset = spor.read_with(&cx, |spor, _| spor.history_options.widths);
    assert_eq!(reset.author, before.author);
    assert_eq!(reset.branch, Column::Branch.range().1, "only that column");
}

#[gpui_kit::test]
fn the_rail_switches_sidebar_and_center_together(cx: &mut TestAppContext) {
    let _lock = isolate();
    let repo = scratch_repo();
    let (mut cx, spor) = open(cx, &repo);
    let state = |cx: &mut VisualTestContext| spor.read_with(cx, |spor, _| (spor.tab, spor.dock));
    assert_eq!(state(&mut cx), (Tab::Changes, Some(DockPanel::Git)));

    // History brings its branch list along.
    let history = bounds_of(&mut cx, "nav-history").center();
    cx.simulate_click(history, Modifiers::none());
    redraw(&mut cx);
    assert_eq!(state(&mut cx), (Tab::History, Some(DockPanel::Branches)));

    // Clicking the open section again hides the sidebar, keeping the view.
    let history = bounds_of(&mut cx, "nav-history").center();
    cx.simulate_click(history, Modifiers::none());
    redraw(&mut cx);
    assert_eq!(state(&mut cx), (Tab::History, None));

    // A shortcut to a view brings its panel back with the sidebar shown.
    cx.dispatch_action(app::ToggleDock);
    redraw(&mut cx);
    assert_eq!(state(&mut cx), (Tab::History, Some(DockPanel::Branches)));
    cx.dispatch_action(app::ShowChanges);
    redraw(&mut cx);
    assert_eq!(state(&mut cx), (Tab::Changes, Some(DockPanel::Git)));
}
