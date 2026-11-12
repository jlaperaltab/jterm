#!/bin/sh
# Compila en release y empaqueta jTerm.app (firma ad-hoc, sin notarizar).
#   UNIVERSAL=1 ./build_app.sh   -> binario para Apple Silicon e Intel (para compartir)
#   VERSION=1.2.0 ./build_app.sh -> número de versión de la app
#   ADHOC=1 ./build_app.sh       -> firma ad-hoc (para compartir con otros Macs)
set -e
cd "$(dirname "$0")"
ARCHS=""
[ -n "$UNIVERSAL" ] && ARCHS="--arch arm64 --arch x86_64"
VERSION="${VERSION:-0.1.0}"
GITHUB_REPO="${GITHUB_REPO:-jotaele/jterm}"   # "usuario/repo" de GitHub donde se publican los releases
swift build -c release $ARCHS
BIN="$(swift build -c release $ARCHS --show-bin-path)/jTerm"
APP=build/jTerm.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/jTerm"
cp Resources/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>jTerm</string>
<key>CFBundleDisplayName</key><string>jTerm</string>
<key>CFBundleIdentifier</key><string>cl.jotaele.jterm</string>
<key>CFBundleIconFile</key><string>AppIcon</string>
<key>CFBundleExecutable</key><string>jTerm</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$VERSION</string>
<key>CFBundleVersion</key><string>$VERSION</string>
<key>JTermUpdateRepo</key><string>$GITHUB_REPO</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSPrincipalClass</key><string>NSApplication</string>
</dict></plist>
PLIST
# Firma con la identidad estable "jTerm Local Signing" (créala una vez con tools/make_signing_cert.sh)
# para que el permiso del Keychain ("Permitir siempre") sobreviva a las recompilaciones.
# Sin ella cae a firma ad-hoc, que cambia en cada build y hace que macOS pida la clave de nuevo.
if [ -z "$ADHOC" ] && security find-certificate -c "jTerm Local Signing" >/dev/null 2>&1; then
  codesign --force --sign "jTerm Local Signing" "$APP"
else
  [ -z "$ADHOC" ] && echo "AVISO: sin certificado 'jTerm Local Signing'; firma ad-hoc (el Keychain volverá a pedir la clave). Ejecuta tools/make_signing_cert.sh"
  if [ -n "$ADHOC" ]; then codesign --force --sign - "$APP"
  else codesign --force --sign - -r='designated => identifier "cl.jotaele.jterm"' "$APP"; fi
fi
echo "Listo: $APP"
