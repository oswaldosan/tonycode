#!/bin/bash
# Compila tonycode y lo instala en ~/Applications/tonycode.app
set -e
cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
  if [ -x "$HOME/.cargo/bin/cargo" ]; then
    source "$HOME/.cargo/env"
  else
    echo "==> Rust no está instalado. Instalando rustup (perfil mínimo)..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    source "$HOME/.cargo/env"
  fi
fi

echo "==> Compilando (la primera vez tarda unos minutos)..."
cargo build --release

APP="$HOME/Applications/tonycode.app"
echo "==> Creando $APP"
mkdir -p "$HOME/Applications"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/tonycode "$APP/Contents/MacOS/tonycode"

ICONSET="$(mktemp -d)/tonycode.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  sips -z $s $s assets/icon.png --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s*2)); sips -z $d $d assets/icon.png --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/tonycode.icns" || true

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>tonycode</string>
  <key>CFBundleDisplayName</key><string>tonycode</string>
  <key>CFBundleIdentifier</key><string>com.oswaldo.tonycode</string>
  <key>CFBundleExecutable</key><string>tonycode</string>
  <key>CFBundleIconFile</key><string>tonycode</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>tonycode usa el micrófono para que Claude Code transcriba lo que dictas (/voice).</string>
</dict>
</plist>
PLIST

echo "==> Listo: $APP"
if [ "$1" != "--no-open" ]; then open "$APP"; fi
