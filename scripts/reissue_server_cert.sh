#!/usr/bin/env bash
set -euo pipefail

# Reissue the TLS server certificate with custom SANs so rustls hostname check passes.
#
# Usage:
#   scripts/reissue_server_cert.sh <name-or-ip> [more names/ips...]
#
# Examples:
#   scripts/reissue_server_cert.sh 10.74.253.40
#   scripts/reissue_server_cert.sh localhost 127.0.0.1 10.74.253.40
#
# Requires: openssl, existing CA at certs/ca.crt and certs/ca.key

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")"/.. && pwd)"
CERT_DIR="$ROOT_DIR/certs"
CA_CRT="$CERT_DIR/ca.crt"
CA_KEY="$CERT_DIR/ca.key"
SERVER_KEY="$CERT_DIR/server.key"
SERVER_CSR="$CERT_DIR/server.csr"
SERVER_CRT="$CERT_DIR/server.crt"
OPENSSL_CNF="$CERT_DIR/server_openssl.cnf"

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <name-or-ip> [more names/ips...]" >&2
  exit 1
fi
if [[ ! -f "$CA_CRT" || ! -f "$CA_KEY" ]]; then
  echo "Missing CA files in $CERT_DIR (need ca.crt and ca.key)" >&2
  exit 1
fi

# Generate a key if it does not exist
if [[ ! -f "$SERVER_KEY" ]]; then
  echo "[reissue] Generating server key: $SERVER_KEY"
  openssl genrsa -out "$SERVER_KEY" 2048 >/dev/null 2>&1
fi

CN="$1"
shift || true

echo "[reissue] Writing OpenSSL config with CN=$CN"
{
  echo "[ req ]"
  echo "default_bits = 2048"
  echo "prompt = no"
  echo "default_md = sha256"
  echo "req_extensions = req_ext"
  echo "distinguished_name = dn"
  echo
  echo "[ dn ]"
  echo "CN = $CN"
  echo
  echo "[ req_ext ]"
  echo "subjectAltName = @alt_names"
  echo
  echo "[ alt_names ]"
  idx=1
  # Add provided CN first to SAN list
  if [[ "$CN" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "IP.$idx = $CN"; idx=$((idx+1))
  else
    echo "DNS.$idx = $CN"; idx=$((idx+1))
  fi
  # Always include localhost and 127.0.0.1
  echo "DNS.$idx = localhost"; idx=$((idx+1))
  echo "IP.$idx = 127.0.0.1"; idx=$((idx+1))
  # Add the rest of the arguments
  for name in "$@"; do
    if [[ "$name" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
      echo "IP.$idx = $name"; idx=$((idx+1))
    else
      echo "DNS.$idx = $name"; idx=$((idx+1))
    fi
  done
} > "$OPENSSL_CNF"

echo "[reissue] Creating CSR"
openssl req -new -key "$SERVER_KEY" -out "$SERVER_CSR" -config "$OPENSSL_CNF" >/dev/null 2>&1

echo "[reissue] Signing server certificate with CA"
openssl x509 -req -in "$SERVER_CSR" -CA "$CA_CRT" -CAkey "$CA_KEY" -CAcreateserial \
  -out "$SERVER_CRT" -days 365 -sha256 -extensions req_ext -extfile "$OPENSSL_CNF" >/dev/null 2>&1

echo "✅ Reissued $SERVER_CRT with SANs from $OPENSSL_CNF"
echo "   CN: $CN"
echo "   Now restart the server using:"
echo "     cargo run -p rura_server -- --tls-cert certs/server.crt --tls-key certs/server.key --port 8443 --debug-io"

