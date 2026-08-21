#!/bin/bash
# IH Phase-2 attack matrix vs headless accelerator-server (deny-by-default)
H="http://127.0.0.1:59833"
probe() { # label, expected, curl args...
  local label="$1"; shift
  local out; out=$(curl -s -m 4 -o /tmp/ih-body -w "%{http_code}" "$@" 2>/dev/null)
  printf "%-52s %s  %s\n" "$label" "$out" "$(head -c 90 /tmp/ih-body | tr '\n' ' ')"
}

echo "== HOST GUARD (expect 403 invalid_host on every line) =="
probe "Host: evil.com:59833 (rebinding)"        -H "Host: evil.com:59833" "$H/health"
probe "Host: localhost.evil.com:59833"          -H "Host: localhost.evil.com:59833" "$H/health"
probe "Host: 127.0.0.1:59834 (wrong port)"      -H "Host: 127.0.0.1:59834" "$H/health"
probe "Host: [::1]:59833 (v6 literal)"          -H "Host: [::1]:59833" "$H/health"
probe "Host: 127.0.0.1..:59833 (multi-dot)"     -H "Host: 127.0.0.1..:59833" "$H/health"
probe "Host: user@127.0.0.1:59833 (userinfo)"   -H "Host: user@127.0.0.1:59833" "$H/health"
probe "Host: 0x7f000001:59833 (hex IP)"         -H "Host: 0x7f000001:59833" "$H/health"
probe "Host: 2130706433:59833 (decimal IP)"     -H "Host: 2130706433:59833" "$H/health"
probe "no Host at all (HTTP/1.0)"               --http1.0 -H "Host:" "$H/health"
probe "Host: LOCALHOST:59833 (uppercase)"       -H "Host: LOCALHOST:59833" "$H/health"

echo; echo "== /HEALTH ORIGIN TIERING =="
probe "health, Origin https://evil.com (minimal?)" -H "Origin: https://evil.com" "$H/health"
probe "health, Origin null"                        -H "Origin: null" "$H/health"

echo; echo "== /PROVE AUTHZ (headless denies unapproved origins) =="
P="$H/prove"
probe "prove, no Origin (auto-approve path)"    -X POST -H "Content-Type: application/json" --data '{}' "$P"
probe "prove, Origin https://evil.com"          -X POST -H "Origin: https://evil.com" -H "Content-Type: application/json" --data '{}' "$P"
probe "prove, Origin null"                      -X POST -H "Origin: null" -H "Content-Type: application/json" --data '{}' "$P"
probe "prove, Origin garbage :://x"             -X POST -H "Origin: :://x" -H "Content-Type: application/json" --data '{}' "$P"
probe "prove, Origin http://LOCALHOST:9999"     -X POST -H "Origin: http://LOCALHOST:9999" -H "Content-Type: application/json" --data '{}' "$P"
probe "prove, Origin http://evil.localhost"     -X POST -H "Origin: http://evil.localhost" -H "Content-Type: application/json" --data '{}' "$P"

echo; echo "== BODY LIMITS =="
probe "prove CL=99999999 (declared oversize)"   -X POST -H "Content-Length: 99999999" "$P"
probe "prove CL=abc (malformed)"                -X POST -H "Content-Length: abc" "$P"
probe "prove CL=5,6 (conflicting)"              -X POST -H "Content-Length: 5,6" "$P"

echo; echo "== VERSION HEADER =="
probe "prove x-aztec-version ../../etc"         -X POST -H "x-aztec-version: ../../../etc/passwd" --data '{}' "$P"
probe "prove x-aztec-version 1.0.0"             -X POST -H "x-aztec-version: 1.0.0" --data '{}' "$P"
