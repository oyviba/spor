//! The Stop hook (#103): block a failing finish, bounded, and log it.

use spor::hooks::stop;
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("spor-hooks-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join(".spor")).unwrap();
    d
}

#[test]
fn failing_checks_block_the_stop_until_the_tries_run_out() {
    let d = dir("fail");
    std::fs::write(d.join(".spor/checks"), "# lint\necho broken >&2; exit 1\n").unwrap();
    let (log, state) = (d.join("log"), d.join("tries"));

    for n in 1..=2 {
        let reply = stop(&d, &log, &state, 2).expect("blocked");
        assert!(reply.contains(r#""decision":"block""#), "{reply}");
        assert!(reply.contains("broken"), "{reply}");
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains(&format!(r#""tries":{n}"#)), "{text}");
    }
    // The third time it may stop, and says it gave up.
    assert!(stop(&d, &log, &state, 2).is_none());
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains(r#""kept_going":false"#), "{text}");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn passing_checks_let_it_finish_and_reset_the_count() {
    let d = dir("pass");
    std::fs::write(d.join(".spor/checks"), "true\n").unwrap();
    let (log, state) = (d.join("log"), d.join("tries"));
    std::fs::write(&state, "1").unwrap();
    assert!(stop(&d, &log, &state, 2).is_none());
    assert!(!state.exists());
    assert!(std::fs::read_to_string(&log)
        .unwrap()
        .contains(r#""state":"passed""#));
    // No checks file: nothing to run, nothing logged.
    let none = dir("none");
    std::fs::remove_dir_all(none.join(".spor")).unwrap();
    assert!(stop(&none, &none.join("log"), &none.join("t"), 2).is_none());
    assert!(!none.join("log").exists());
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&none);
}
