#!/usr/bin/env bash
set -euo pipefail
HIER="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VENV="$HIER/.venv"
if [ ! -d "$VENV" ]; then
  python3 -m venv "$VENV"
fi
"$VENV/bin/pip" install --upgrade pip >/dev/null
"$VENV/bin/pip" install -r "$HIER/requirements.txt"
echo "venv bereit: $VENV"
"$VENV/bin/python" -c "import cv2, numpy, pytesseract, requests, psycopg; print('Importe ok:', cv2.__version__)"
