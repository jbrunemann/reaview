#!/bin/sh
# Build Reaview.app so Finder "Open With" and drag-to-Dock work.
# UNIVERSAL=1 builds a fat arm64 + x86_64 binary (needs both rustup targets).
set -eu
cd "$(dirname "$0")/.."
APP=target/Reaview.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
if [ "${UNIVERSAL:-0}" = 1 ]; then
	cargo build --release --target aarch64-apple-darwin
	cargo build --release --target x86_64-apple-darwin
	lipo -create -output "$APP/Contents/MacOS/Reaview" \
		target/aarch64-apple-darwin/release/Reaview target/x86_64-apple-darwin/release/Reaview
else
	cargo build --release
	cp target/release/Reaview "$APP/Contents/MacOS/Reaview"
fi
cp assets/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cat > "$APP/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleExecutable</key><string>Reaview</string>
	<key>CFBundleIdentifier</key><string>dev.reaview.ai</string>
	<key>CFBundleName</key><string>Reaview</string>
	<key>CFBundleDisplayName</key><string>Reaview</string>
	<key>CFBundleIconFile</key><string>AppIcon</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>CFBundleShortVersionString</key><string>0.2.0</string>
	<key>CFBundleVersion</key><string>1</string>
	<key>LSMinimumSystemVersion</key><string>11.0</string>
	<key>NSHighResolutionCapable</key><true/>
	<key>CFBundleDocumentTypes</key>
	<array>
		<dict>
			<key>CFBundleTypeName</key><string>Image</string>
			<key>CFBundleTypeRole</key><string>Viewer</string>
			<key>LSHandlerRank</key><string>Alternate</string>
			<key>LSItemContentTypes</key><array><string>public.image</string></array>
		</dict>
	</array>
</dict>
</plist>
PLIST
# App Sandbox: read-only access to files the user opens, plus folders they grant
# (kept as security-scoped bookmarks, see src/access.rs), and outgoing network
# connections for "Check for Updates…" (see src/update.rs).
ENTITLEMENTS=target/Reaview.entitlements
cat > "$ENTITLEMENTS" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>com.apple.security.app-sandbox</key><true/>
	<key>com.apple.security.files.user-selected.read-only</key><true/>
	<key>com.apple.security.files.bookmarks.app-scope</key><true/>
	<key>com.apple.security.network.client</key><true/>
</dict>
</plist>
PLIST
codesign --force --options runtime --entitlements "$ENTITLEMENTS" --sign - "$APP" >/dev/null
echo "Built $APP"
