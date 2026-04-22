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
