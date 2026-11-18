#!/bin/sh
# python-licenses.sh fails when a Python dependency of the project in $1 is outside Apache-2.0 and the CNCF allowlist.
set -eu
cd "$1"
uv sync --locked --quiet
# certifi: CNCF blanket exception, fetched at install time.
# bidict (MPL-2.0), zope.event and zope.interface (ZPL-2.1): pulled in by locust, no CNCF exception yet.
# pip-licenses, prettytable, wcwidth: this check's own tooling.
uv run --quiet --with pip-licenses pip-licenses --partial-match \
  "--allow-only=MIT;BSD;Apache;ISC;Zlib;PSF" \
  --ignore-packages certifi bidict zope.event zope.interface pip-licenses prettytable wcwidth
