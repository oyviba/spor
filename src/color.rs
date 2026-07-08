/// Color assignment: branch family (prefix before /) determines hue,
/// full branch name varies lightness so siblings are distinguishable.
///
/// Families we care about:
///   feat/*  -> blue-ish
///   fix/*, bug/* -> red-ish
///   chore/* -> grey-ish
///   everything else -> hashed hue

pub type Rgb = (u8, u8, u8);

pub fn color_for(family: &str, full_name: &str) -> Rgb {
    // Hue seed per family. "_" is the fallback for orphan lanes.
    let (hue, sat) = match family {
        "feat" | "feature" => (210.0, 0.65), // blue
        "fix" | "bug" => (0.0, 0.70),        // red
        "chore" => (0.0, 0.0),               // grey
        "docs" => (280.0, 0.50),             // purple
        "refactor" => (160.0, 0.55),         // teal
        "test" => (50.0, 0.60),              // yellow-green
        "main" | "master" | "trunk" => (130.0, 0.55), // green
        "_" => (0.0, 0.0),                   // unknown/orphan: grey
        other => (hash_hue(other), 0.55),    // unknown prefix: stable hash
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
}
