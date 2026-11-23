#!/bin/sh
# Compila jTerm (Rust) en release y empaqueta jTerm.app (firma ad-hoc, sin notarizar).
# Genera la MISMA app que la versión Swift (mismo identificador, mismo nombre de ejecutable y mismas claves del
# Info.plist), así que una instalación anterior se actualiza a esta desde su propio "Buscar actualizaciones…".
#   UNIVERSAL=1 ./build_app.sh   -> binario para Apple Silicon e Intel (para compartir; necesita los dos targets de Rust)
#   VERSION=0.3.0 ./build_app.sh -> número de versión de la app (por defecto, el de Cargo.toml)
#   ADHOC=1 ./build_app.sh       -> firma ad-hoc (para compartir con otros Macs)
set -e
cd "$(dirname "$0")"
[ -d /opt/homebrew/opt/rustup/bin ] && export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
VERSION="${VERSION:-$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')}"
GITHUB_REPO="${GITHUB_REPO:-jlaperaltab/jterm}"   # "usuario/repo" de GitHub donde se publican los releases
BIN_NAME=jterm

if [ -n "$UNIVERSAL" ]; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  cargo build --release -p jterm-app --target aarch64-apple-darwin
  cargo build --release -p jterm-app --target x86_64-apple-darwin
  mkdir -p target/universal
  lipo -create -output target/universal/$BIN_NAME \
    target/aarch64-apple-darwin/release/$BIN_NAME target/x86_64-apple-darwin/release/$BIN_NAME
  BIN="target/universal/$BIN_NAME"
else
  cargo build --release -p jterm-app
  BIN="target/release/$BIN_NAME"
fi

APP=build/jTerm.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/jTerm"
cp ../Resources/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
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
# Misma política de firma que la versión Swift: con la identidad estable "jTerm Local Signing" el permiso del Keychain
# sobrevive a las recompilaciones; si no, firma ad-hoc con un requisito fijo (solo el identificador), así el Keychain
# reconoce cada versión nueva como la misma app y no vuelve a pedir la clave tras actualizar.
if [ -z "$ADHOC" ] && security find-certificate -c "jTerm Local Signing" >/dev/null 2>&1; then
  codesign --force --sign "jTerm Local Signing" "$APP"
else
  [ -z "$ADHOC" ] && echo "AVISO: sin certificado 'jTerm Local Signing'; firma ad-hoc. Ejecuta ../tools/make_signing_cert.sh"
  codesign --force --sign - -r='designated => identifier "cl.jotaele.jterm"' "$APP"
fi
echo "Listo: $APP (versión $VERSION, repositorio de actualizaciones: $GITHUB_REPO)"
