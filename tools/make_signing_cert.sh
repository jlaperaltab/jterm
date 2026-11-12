#!/bin/sh
# Crea UNA VEZ un certificado de firma autofirmado ("jTerm Local Signing") en tu llavero de inicio.
# Con una identidad estable, el permiso "Permitir siempre" del Keychain sobrevive a las recompilaciones
# (la firma ad-hoc cambia en cada build y por eso macOS vuelve a pedir la clave).
# No toca servidores ni AWS: solo crea el certificado local.
set -e
NAME="jTerm Local Signing"
KC="$HOME/Library/Keychains/login.keychain-db"
if security find-certificate -c "$NAME" "$KC" >/dev/null 2>&1; then
  echo "Ya existe '$NAME'."; exit 0
fi
T="$(mktemp -d)"; trap 'rm -rf "$T"' EXIT
cat > "$T/o.cnf" <<CNF
[req]
distinguished_name=dn
x509_extensions=ext
prompt=no
[dn]
CN=$NAME
[ext]
basicConstraints=critical,CA:false
keyUsage=critical,digitalSignature
extendedKeyUsage=critical,codeSigning
CNF
openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -keyout "$T/k.pem" -out "$T/c.pem" -config "$T/o.cnf" 2>/dev/null
openssl pkcs12 -export -inkey "$T/k.pem" -in "$T/c.pem" -out "$T/id.p12" -passout pass:jterm -legacy 2>/dev/null \
  || openssl pkcs12 -export -inkey "$T/k.pem" -in "$T/c.pem" -out "$T/id.p12" -passout pass:jterm
security import "$T/id.p12" -k "$KC" -P jterm -T /usr/bin/codesign
echo "Listo. Ahora ejecuta ./build_app.sh"
