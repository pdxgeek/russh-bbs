#!/usr/bin/env bash
# Make a throwaway CA and a server certificate it signs, for testing Postgres with TLS required
# (D31). A server certificate that is its own trust anchor is rejected by rustls
# (CaUsedAsEndEntity), so, like a real deployment, the server certificate is signed by a CA and
# clients trust the CA file. Usage: postgres-tls-certs.sh <output-dir> [host ...]
# Writes ca.crt (give this to clients), server.crt and server.key. Nothing here is a secret:
# the keys are generated fresh, live in <output-dir>, and are for tests only.
set -euo pipefail
out="${1:?usage: $0 <output-dir> [host ...]}"
shift || true
hosts=("${@:-localhost}")
mkdir -p "$out"
san="DNS:localhost,IP:127.0.0.1"
for h in "${hosts[@]}"; do san="$san,DNS:$h"; done

openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 7 \
    -subj "/CN=russh-bbs test CA" \
    -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" \
    -keyout "$out/ca.key" -out "$out/ca.crt" 2>/dev/null
openssl req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
    -subj "/CN=localhost" -keyout "$out/server.key" -out "$out/server.csr" 2>/dev/null
printf 'basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\nsubjectAltName=%s\n' "$san" > "$out/server.ext"
openssl x509 -req -in "$out/server.csr" -CA "$out/ca.crt" -CAkey "$out/ca.key" -CAcreateserial \
    -days 7 -extfile "$out/server.ext" -out "$out/server.crt" 2>/dev/null
chmod 600 "$out/server.key" "$out/ca.key"
chmod 644 "$out/server.crt" "$out/ca.crt"
echo "wrote $out/ca.crt $out/server.crt $out/server.key"
