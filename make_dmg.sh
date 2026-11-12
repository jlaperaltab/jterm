#!/bin/sh
# Genera dist/jTerm-<versión>.dmg con la app universal (Apple Silicon + Intel) para compartir.
#   VERSION=0.2.0 ./make_dmg.sh
set -e
cd "$(dirname "$0")"
VERSION="${VERSION:-0.1.0}"
export VERSION
# Para compartir: firma ad-hoc. Un certificado autofirmado propio no es de confianza en otros Macs.
ADHOC=1 UNIVERSAL=1 ./build_app.sh

STAGE=$(mktemp -d /tmp/jterm-dmg.XXXXXX)
trap 'rm -rf "$STAGE"' EXIT
cp -R build/jTerm.app "$STAGE/jTerm.app"
ln -s /Applications "$STAGE/Aplicaciones"
cat > "$STAGE/LEEME.txt" <<TXT
jTerm $VERSION — cómo instalarlo

1. Arrastra jTerm a la carpeta Aplicaciones.

2. La primera vez, macOS dirá que no puede verificar la app, porque no está
   notarizada por Apple. Es normal. Para abrirla:

   • macOS 15 o posterior: abre Ajustes del Sistema ▸ Privacidad y seguridad,
     baja hasta el aviso “jTerm fue bloqueada” y pulsa “Abrir de todos modos”.
   • macOS 13 o 14: clic derecho sobre jTerm ▸ Abrir ▸ Abrir.
   • Alternativa en Terminal:
       xattr -dr com.apple.quarantine /Applications/jTerm.app

   Solo hace falta la primera vez.

Requisitos: macOS 13 o superior. Funciona en Macs con Apple Silicon e Intel.

Tus datos se guardan en ~/Library/Application Support/jTerm y las contraseñas
en el Keychain de macOS. Cada persona tiene los suyos.
TXT

# --- Fondo con instrucciones, y ventana con iconos grandes (app -> Aplicaciones) ---
mkdir -p "$STAGE/.background"
swift tools/make_dmg_bg.swift "$STAGE/.background/bg.png"
cp Resources/AppIcon.icns "$STAGE/.VolumeIcon.icns"

mkdir -p dist
OUT="dist/jTerm-$VERSION.dmg"
VOL="jTerm $VERSION"
RW="$(mktemp -u /tmp/jterm-rw.XXXXXX).dmg"
rm -f "$OUT"
hdiutil detach "/Volumes/$VOL" >/dev/null 2>&1 || true
hdiutil create -volname "$VOL" -srcfolder "$STAGE" -ov -format UDRW -fs HFS+ "$RW" >/dev/null
hdiutil attach "$RW" -noverify -noautoopen >/dev/null
command -v SetFile >/dev/null && SetFile -a C "/Volumes/$VOL" || true
if osascript <<OSA
tell application "Finder"
  tell disk "$VOL"
    open
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set the bounds of container window to {200, 120, 860, 520}
    set opts to the icon view options of container window
    set arrangement of opts to not arranged
    set icon size of opts to 128
    set text size of opts to 13
    set background picture of opts to file ".background:bg.png"
    set position of item "jTerm.app" of container window to {165, 200}
    set position of item "Aplicaciones" of container window to {495, 200}
    set position of item "LEEME.txt" of container window to {330, 335}
    close
    open
    update without registering applications
    delay 2
    close
  end tell
end tell
OSA
then :; else echo "AVISO: no se pudo estilizar la ventana (permite a Terminal controlar Finder en Ajustes ▸ Privacidad ▸ Automatización)"; fi
sync; sleep 1
hdiutil detach "/Volumes/$VOL" >/dev/null || { sleep 2; hdiutil detach -force "/Volumes/$VOL" >/dev/null; }
hdiutil convert "$RW" -format UDZO -imagekey zlib-level=9 -o "$OUT" >/dev/null
rm -f "$RW"
( cd dist && shasum -a 256 "jTerm-$VERSION.dmg" > "jTerm-$VERSION.dmg.sha256" )
echo "Listo: $OUT ($(du -h "$OUT" | cut -f1))"
