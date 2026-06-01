#!/usr/bin/env bash
set -euo pipefail

NET_CONN="${DORM_NET_CONN:-Wired connection 1}"
NET_IF="${DORM_NET_IF:-enp44s0}"

ENTRY_URL="http://connectivitycheck.gstatic.com/generate_204"

COOKIE_FILE="/tmp/dorm-net-cookies.txt"
HEADERS_FILE="/tmp/dorm-net-headers.txt"
PAGE_BODY="/tmp/dorm-net-page.html"
PAYLOAD_FILE="/tmp/dorm-net-login-payload.txt"
LOGIN_BODY="/tmp/dorm-net-login-response.html"
LOG_FILE="/tmp/dorm-net-login-$(date +%F-%H%M%S).log"

DORM_WIFI_USER="${DORM_WIFI_USER:?Missing DORM_WIFI_USER}"
DORM_WIFI_PASS="${DORM_WIFI_PASS:?Missing DORM_WIFI_PASS}"

PYTHON_BIN="$(command -v python3 || command -v python || true)"
if [ -z "$PYTHON_BIN" ]; then
  echo "Missing python/python3."
  exit 1
fi

exec > >(tee -a "$LOG_FILE") 2>&1

net_curl() {
  curl -4 --interface "$NET_IF" "$@"
}

internet_ok() {
  net_curl -sS --max-time 8 https://example.com >/dev/null 2>&1
}

echo "[0/6] Dorm Ethernet login"
echo "interface: $NET_IF"
echo "connection: $NET_CONN"
echo "log: $LOG_FILE"

echo "[1/6] Bringing Ethernet up..."
sudo nmcli connection up "$NET_CONN" || true

echo "[2/6] Checking internet through Ethernet..."
if internet_ok; then
  echo "Ethernet internet already works."
  exit 0
fi

rm -f "$COOKIE_FILE" "$HEADERS_FILE" "$PAGE_BODY" "$PAYLOAD_FILE" "$LOGIN_BODY"
: >"$HEADERS_FILE"
: >"$PAGE_BODY"

echo "[3/6] Loading captive portal page..."

net_curl -k -sS -L \
  --max-time 30 \
  -D "$HEADERS_FILE" \
  -o "$PAGE_BODY" \
  -c "$COOKIE_FILE" \
  -b "$COOKIE_FILE" \
  -H "user-agent: Mozilla/5.0" \
  "$ENTRY_URL" || true

if ! grep -qi 'id="login-user"' "$PAGE_BODY"; then
  echo "Could not find login-user form."
  echo "Debug:"
  grep -iE 'login|form|input|dst|DeviceInfo|Location:' "$HEADERS_FILE" "$PAGE_BODY" | head -120 || true
  echo "Saved: $LOG_FILE $HEADERS_FILE $PAGE_BODY"
  exit 2
fi

echo "[4/6] Building login payload..."

LOGIN_ACTION="$(
  "$PYTHON_BIN" - "$PAGE_BODY" "$PAYLOAD_FILE" "$DORM_WIFI_USER" "$DORM_WIFI_PASS" <<'PY'
import re, sys
from html.parser import HTMLParser
from urllib.parse import urlencode, urljoin

page, payload_path, username, password = sys.argv[1:5]
html = open(page, "r", encoding="utf-8", errors="ignore").read()

class Parser(HTMLParser):
    def __init__(self):
        super().__init__()
        self.in_form = False
        self.action = ""
        self.fields = []

    def handle_starttag(self, tag, attrs):
        attrs = {k.lower(): (v if v is not None else "") for k, v in attrs}
        tag = tag.lower()

        if tag == "form" and attrs.get("id") == "login-user":
            self.in_form = True
            self.action = attrs.get("action", "")

        if self.in_form and tag == "input":
            name = attrs.get("name", "")
            if not name:
                return
            typ = attrs.get("type", "text").lower()
            if typ in {"submit", "button", "reset", "image", "file"}:
                return
            self.fields.append((name, attrs.get("value", "")))

    def handle_endtag(self, tag):
        if tag.lower() == "form" and self.in_form:
            self.in_form = False

p = Parser()
p.feed(html)

if not p.action:
    raise SystemExit("No form action found")

pairs = []
seen = set()

for k, v in p.fields:
    lk = k.lower()
    if lk == "username":
        v = username
    elif lk == "password":
        v = password
    pairs.append((k, v))
    seen.add(lk)

if "username" not in seen:
    pairs.append(("username", username))
if "password" not in seen:
    pairs.append(("password", password))
if "popup" not in seen:
    pairs.append(("popup", "true"))

open(payload_path, "w", encoding="utf-8").write(urlencode(pairs))

print(p.action)
PY
)"

echo "login action: $LOGIN_ACTION"
echo "payload:"
sed -E 's/(password=)[^&]*/\1***/g' "$PAYLOAD_FILE"

echo "[5/6] Posting login form..."

net_curl -k -sS -L \
  --max-time 30 \
  -o "$LOGIN_BODY" \
  -c "$COOKIE_FILE" \
  -b "$COOKIE_FILE" \
  -X POST \
  -H "content-type: application/x-www-form-urlencoded" \
  -H "origin: https://login.net.vn" \
  -H "referer: https://login.net.vn/login" \
  -H "user-agent: Mozilla/5.0" \
  --data-binary "@$PAYLOAD_FILE" \
  "$LOGIN_ACTION" || true

echo "[6/6] Testing internet..."
sleep 2

if internet_ok; then
  echo "Ethernet internet works."
  exit 0
fi

echo "Login posted, but internet still failed."
echo "Saved log: $LOG_FILE"
echo "Saved page: $PAGE_BODY"
echo "Saved response: $LOGIN_BODY"
exit 1
