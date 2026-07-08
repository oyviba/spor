//! Color assignment: branch family (prefix before /) determines hue,
//! full branch name varies lightness so siblings are distinguishable.
//!
//! Families we care about:
//!   feat/*  -> blue-ish
//!   fix/*, bug/* -> red-ish
//!   chore/* -> grey-ish
//!   everything else -> hashed hue

pub type Rgb = (u8, u8, u8);

/// Extract the family (color group) from a branch name: the prefix before the
/// first `/`, or the whole name if there is none. A leading segment is only
/// treated as a remote prefix when it matches an actual configured remote, so
/// local `feat/x` keeps family "feat" while `origin/feat/x` also maps to "feat".
pub fn branch_family<'a>(name: &'a str, remotes: &[String]) -> &'a str {
    let name = match name.split_once('/') {
        Some((first, rest)) if remotes.iter().any(|r| r == first) => rest,
        _ => name,
    };
    match name.split_once('/') {
        Some((prefix, _)) => prefix,
        None => name,
    }
}

pub fn color_for(family: &str, full_name: &str) -> Rgb {
    // Hue seed per family. "_" is the fallback for orphan lanes.
    let (hue, sat) = match family {
        "feat" | "feature" => (210.0, 0.65),          // blue
        "fix" | "bug" => (0.0, 0.70),                 // red
        "chore" => (0.0, 0.0),                        // grey
        "docs" => (280.0, 0.50),                      // purple
        "refactor" => (160.0, 0.55),                  // teal
        "test" => (50.0, 0.60),                       // yellow-green
        "main" | "master" | "trunk" => (130.0, 0.55), // green
        "_" => (0.0, 0.0),                            // unknown/orphan: grey
        other => (hash_hue(other), 0.55),             // unknown prefix: stable hash
    };

    // Lightness varies per branch so `feat/login` and `feat/signup` differ.
    let lightness = 0.45 + ((hash(full_name) % 30) as f32) / 100.0; // 0.45..0.75
    hsl_to_rgb(hue, sat, lightness)
}

fn hash(s: &str) -> u32 {
    // FNV-1a, good enough for color stability.
    let mut h: u32 = 2166136261;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

fn hash_hue(s: &str) -> f32 {
    (hash(s) % 360) as f32
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> Rgb {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h6 = h / 60.0;
    let x = c * (1.0 - (h6 % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match h6 as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    (
        ((r1 + m) * 255.0).clamp(0.0, 255.0) as u8,
        ((g1 + m) * 255.0).clamp(0.0, 255.0) as u8,
        ((b1 + m) * 255.0).clamp(0.0, 255.0) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> Vec<String> {
        vec!["origin".to_string()]
    }

    #[test]
    fn local_branch_keeps_its_prefix() {
        assert_eq!(branch_family("feat/login", &origin()), "feat");
        assert_eq!(branch_family("bug/crash", &origin()), "bug");
    }

    #[test]
    fn remote_prefix_is_stripped_before_family() {
        assert_eq!(branch_family("origin/feat/login", &origin()), "feat");
        assert_eq!(branch_family("origin/main", &origin()), "main");
    }

    #[test]
    fn unprefixed_branch_is_its_own_family() {
        assert_eq!(branch_family("main", &origin()), "main");
    }

    #[test]
    fn non_remote_first_segment_is_not_stripped() {
        // No remote called "feat" — the prefix is the family, not a remote.
        assert_eq!(branch_family("feat/login", &[]), "feat");
    }

    #[test]
    fn deterministic() {
        assert_eq!(
            color_for("feat", "feat/login"),
            color_for("feat", "feat/login")
        );
    }

    #[test]
    fn siblings_in_same_family_differ() {
        assert_ne!(
            color_for("feat", "feat/login"),
            color_for("feat", "feat/signup")
        );
    }

    #[test]
    fn family_hue_dominates() {
        let (r, _, b) = color_for("feat", "feat/login");
        assert!(b > r, "feat should be blue-ish");
        let (r, _, b) = color_for("fix", "fix/crash");
        assert!(r > b, "fix should be red-ish");
        let (r, g, b) = color_for("main", "main");
        assert!(g > r && g > b, "main should be green-ish");
    }

    #[test]
    fn chore_and_orphan_are_grey() {
        for (family, name) in [("chore", "chore/deps"), ("_", "whatever")] {
            let (r, g, b) = color_for(family, name);
            assert_eq!(r, g, "{family} should be grey");
            assert_eq!(g, b, "{family} should be grey");
        }
    }

    #[test]
    fn unknown_family_is_stable_and_not_black() {
        let a = color_for("experiment", "experiment/foo");
        assert_eq!(a, color_for("experiment", "experiment/foo"));
        assert_ne!(a, (0, 0, 0));
    }

    #[test]
    fn same_family_shares_hue_different_lightness() {
        let a = color_for("feat", "feat/login");
        let b = color_for("feat", "feat/signup");
        // Not asserting exact values — just that the mapping is stable and
        // the two names don't collapse to the identical color.
        assert_eq!(a, color_for("feat", "feat/login"));
        assert_ne!(a, b);
    }
}
