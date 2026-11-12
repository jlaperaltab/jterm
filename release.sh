#!/bin/sh
# Compila el DMG y lo publica como Release de GitHub (necesita `gh auth login` y el repo ya creado).
#   ./release.sh 0.2.0            -> tag v0.2.0, sube dist/jTerm-0.2.0.dmg y su .sha256
#   NOTES="Cambios..." ./release.sh 0.2.0
set -e
cd "$(dirname "$0")"
VERSION="${1:?Uso: ./release.sh <versión>   (ej. 0.2.0)}"
export VERSION
./make_dmg.sh
gh release create "v$VERSION" "dist/jTerm-$VERSION.dmg" "dist/jTerm-$VERSION.dmg.sha256" \
  --title "jTerm $VERSION" --notes "${NOTES:-jTerm $VERSION}"
echo "Publicado v$VERSION"
