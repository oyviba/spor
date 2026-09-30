#!/usr/bin/env bash
# Build Spor.app — the native spor window as a double-clickable macOS app.
#
#   scripts/bundle-macos.sh              # this Mac's architecture
#   scripts/bundle-macos.sh --universal  # Apple Silicon + Intel in one binary
#
# Output: target/macos/Spor.app and target/macos/Spor-<version>-macos.zip
# Needs Xcode command line tools (sips, iconutil, lipo, codesign, ditto).
set -euo pipefail

cd "$(dirname "$0")/.."

universal=false
for arg in "$@"; do
    case "$arg" in
        --universal) universal=true ;;
        *) echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done

if [[ "$(uname)" != "Darwin" ]]; then
    echo "bundle-macos.sh must run on macOS" >&2
    exit 1
fi

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
out=target/macos
app="$out/Spor.app"

if $universal; then
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    for t in aarch64-apple-darwin x86_64-apple-darwin; do
        cargo build --release --locked --features gui --bin spor-app --target "$t"
    done
    bin="$out/spor-app-universal"
    mkdir -p "$out"
    lipo -create -output "$bin" \
        target/aarch64-apple-darwin/release/spor-app \
        target/x86_64-apple-darwin/release/spor-app
else
    cargo build --release --locked --features gui --bin spor-app
    bin=target/release/spor-app
fi

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/spor-app"

# AppIcon.icns from the 1024px master.
iconset="$out/AppIcon.iconset"
rm -rf "$iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
    sips -z $size $size assets/icon.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double assets/icon.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"
rm -rf "$iconset"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Spor</string>
    <key>CFBundleDisplayName</key>
    <string>Spor</string>
    <key>CFBundleIdentifier</key>
    <string>io.github.oyviba.spor</string>
    <key>CFBundleExecutable</key>
    <string>spor-app</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${version}</string>
    <key>CFBundleVersion</key>
    <string>${version}</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.developer-tools</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
</dict>
</plist>
PLIST

# Ad-hoc signature: enough for Apple Silicon to run a locally built app. A
# downloaded copy still needs Developer ID signing + notarization to open
# without the Gatekeeper prompt.
codesign --force --sign - "$app"

zip="$out/Spor-${version}-macos.zip"
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"

echo "built $app"
echo "      $zip"
