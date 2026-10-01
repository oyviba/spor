# Bundled fonts

Used by the native app (`spor-app`), embedded at compile time.

- **Inter** 4.1 (Regular, SemiBold) — <https://rsms.me/inter/>, SIL Open Font
  License 1.1 (`Inter-LICENSE.txt`).
- **JetBrains Mono** 2.304 (Regular) — <https://www.jetbrains.com/lp/mono/>,
  SIL Open Font License 1.1 (`JetBrainsMono-OFL.txt`).

Both are subset to drop their Private Use Area glyphs (U+E000–F8FF), where
the Phosphor icon font lives; otherwise those glyphs would shadow icons:

```sh
pyftsubset Inter-Regular.ttf --unicodes="U+0000-DFFF,U+F900-FFFF,U+10000-1FFFF" \
  --layout-features='*' --glyph-names --notdef-outline \
  --name-IDs='*' --name-languages='*' --output-file=Inter-Regular.ttf
```

Icons come from the `egui-phosphor` crate (Phosphor Icons, MIT).
