#!/usr/bin/env bash
# Run the process-server as a long-lived SERVICE on a fixed port.
#
# This is how prism reliably invokes the python: start the service ONCE here,
# then point prism's rest address (host:port) at it. prism does NOT spawn or
# babysit this process — spawning it inline and reading the port off piped
# stdout (through `uv run` + the libpython re-exec) is the fragile, rebuild-prone
# path we're avoiding. A service started once, connected to by URL, is reliable.
#
#   process-server/serve.sh [PORT]      # default 8765
#
# Prints "process-server listening on PORT" once bound (readiness signal).
set -euo pipefail
cd "$(dirname "$0")"
exec uv run python server.py "${1:-8765}"
