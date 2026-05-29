#!/usr/bin/env python3
"""A minimal process server speaking prism's rest protocol.

Routes (exactly the ones ``RestProcess`` calls, modelled on upstream
``process_bigraph/server/rest.py`` but stdlib-only and wire-exact):

    POST /process/{class}/initialize        body = config        -> "process_id"
    GET  /process/{class}/inputs/{id}                            -> {port: type}
    GET  /process/{class}/outputs/{id}                           -> {port: type}
    POST /process/{class}/update/{id}        body = {state, interval} -> update
    POST /process/{class}/end/{id}                               -> null

``inputs``/``outputs`` return **bigraph-schema type strings**, so prism parses
them back into real ``Schema``s — the typed seam. Threaded so prism's concurrent
``invoke`` pass talks to many instances at once.

Run: ``uv run python server.py [PORT]`` (PORT 0 = pick a free one; the chosen
port is printed as ``process-server listening on PORT`` for a parent to read).
"""

import json
import os
import sys
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from processes import REGISTRY


def _ensure_libpython():
    """libroadrunner's C++ extension links libpython dynamically, but uv's
    standalone CPython doesn't put libpython on the loader path — so `import
    roadrunner` fails with `libpython3.x.so: cannot open shared object file`.
    The dynamic linker reads LD_LIBRARY_PATH at process start, so we set it and
    re-exec ourselves ONCE (guarded). COPASI is unaffected; this just unblocks
    the Tellurium engine. Transparent to a parent reading our stdout."""
    import sysconfig

    libdir = sysconfig.get_config_var("LIBDIR")
    if not libdir:
        return
    current = os.environ.get("LD_LIBRARY_PATH", "")
    if libdir in current.split(os.pathsep):
        return  # already on the path — second pass, don't re-exec
    os.environ["LD_LIBRARY_PATH"] = libdir + (os.pathsep + current if current else "")
    os.execv(sys.executable, [sys.executable, *sys.argv])

INSTANCES = {}  # process_id -> process instance


class Handler(BaseHTTPRequestHandler):
    def _send(self, obj, status=200):
        body = json.dumps(obj).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _body(self):
        n = int(self.headers.get("Content-Length", 0) or 0)
        if not n:
            return None
        return json.loads(self.rfile.read(n) or b"null")

    def _parts(self):
        return self.path.strip("/").split("/")

    def do_GET(self):
        p = self._parts()
        # /process/{class}/inputs|outputs/{id}
        if len(p) == 4 and p[0] == "process" and p[2] in ("inputs", "outputs"):
            inst = INSTANCES.get(p[3])
            if inst is None:
                return self._send(None, 404)
            return self._send(inst.inputs() if p[2] == "inputs" else inst.outputs())
        self._send(None, 404)

    def do_POST(self):
        p = self._parts()
        body = self._body()
        # /process/{class}/initialize
        if len(p) == 3 and p[0] == "process" and p[2] == "initialize":
            cls = REGISTRY.get(p[1])
            if cls is None:
                return self._send({"process-not-found": p[1]}, 404)
            pid = str(uuid.uuid4())
            INSTANCES[pid] = cls(body or {})
            return self._send(pid)
        # /process/{class}/update/{id}
        if len(p) == 4 and p[0] == "process" and p[2] == "update":
            inst = INSTANCES.get(p[3])
            if inst is None:
                return self._send(None, 404)
            return self._send(inst.update(body.get("state"), body.get("interval", 1.0)))
        # /process/{class}/end/{id}
        if len(p) == 4 and p[0] == "process" and p[2] == "end":
            INSTANCES.pop(p[3], None)
            return self._send(None)
        self._send(None, 404)

    def log_message(self, *_args):
        pass  # quiet — prism drives many requests


def main():
    _ensure_libpython()
    # Line-buffer stdout so the "listening" line reaches a parent reading our
    # piped stdout immediately — a block-buffered pipe can otherwise withhold it,
    # hanging a spawner (e.g. the Rust seam test) that waits to read the port.
    sys.stdout.reconfigure(line_buffering=True)
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 0
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"process-server listening on {server.server_address[1]}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
