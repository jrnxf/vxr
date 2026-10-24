#!/usr/bin/env python3
"""Bench server for the embed spike. Binds 127.0.0.1 only.

- Serves web/ and pkg/ with Cache-Control: no-store, brotli when a .br sibling exists.
- /scene.vox maps to the prototype island.
- Simulated network: one RTT per request, then the body paced through one shared
  link (token bucket), so concurrent downloads share the bandwidth like a real link.
  Set with POST /profile?name=none|10mbit|slow4g.
- POST /results?f=<name> appends the body as one line to results/<name>.jsonl.
"""
import http.server
import os
import sys
import threading
import time
import urllib.parse

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.join(HERE, "web")
ALIASES = {
    "/scene.vox": os.path.join(HERE, "..", "..", "prototype", "demo", "scene.vox"),
    "/shader.wgsl": os.path.join(HERE, "src", "shader.wgsl"),
}
PROFILES = {
    "none": (0.0, 0.0),
    "10mbit": (0.040, 10e6),  # 40 ms RTT, 10 Mbit/s down
    "slow4g": (0.150, 1.6e6),  # Lighthouse Slow 4G: 150 ms RTT, 1.6 Mbit/s down
}
TYPES = {
    ".html": "text/html; charset=utf-8",
    ".js": "text/javascript",
    ".wasm": "application/wasm",
    ".webp": "image/webp",
    ".jpg": "image/jpeg",
    ".png": "image/png",
    ".vox": "application/octet-stream",
    ".json": "application/json",
    ".txt": "text/plain; charset=utf-8",
}

state = {"profile": "none"}
link_lock = threading.Lock()
link_free = [0.0]


def pace(nbytes):
    """Reserve link time for nbytes and return when they may be sent."""
    rtt, bps = PROFILES[state["profile"]]
    if bps <= 0:
        return
    with link_lock:
        start = max(time.monotonic(), link_free[0])
        link_free[0] = start + nbytes * 8 / bps
        done = link_free[0]
    delay = done - time.monotonic()
    if delay > 0:
        time.sleep(delay)


class H(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        if os.environ.get("VERBOSE"):
            sys.stderr.write("%s %s\n" % (self.headers.get("Accept-Encoding"), fmt % args))

    def send_plain(self, code, body=b""):
        self.send_response(code)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        u = urllib.parse.urlparse(self.path)
        q = urllib.parse.parse_qs(u.query)
        n = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(n)
        if u.path == "/profile" and not q:
            return self.send_plain(200, state["profile"].encode())
        if u.path == "/profile":
            name = q.get("name", ["none"])[0]
            if name not in PROFILES:
                return self.send_plain(400, b"unknown profile")
            state["profile"] = name
            return self.send_plain(200, name.encode())
        if u.path == "/results":
            f = "".join(c for c in q.get("f", ["results"])[0] if c.isalnum() or c in "-_")
            os.makedirs(os.path.join(HERE, "results"), exist_ok=True)
            with open(os.path.join(HERE, "results", f + ".jsonl"), "ab") as out:
                out.write(body.replace(b"\n", b" ") + b"\n")
            return self.send_plain(200, b"ok")
        if u.path == "/poster":
            with open(os.path.join(ROOT, "poster.webp"), "wb") as out:
                out.write(body)
            return self.send_plain(200, b"ok")
        self.send_plain(404)

    def do_OPTIONS(self):
        self.send_response(204)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "POST, GET")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        u = urllib.parse.urlparse(self.path)
        path = u.path
        if path == "/":
            path = "/index.html"
        if path in ALIASES:
            fp = ALIASES[path]
        elif path.startswith("/pkg/"):
            fp = os.path.join(HERE, "pkg", os.path.basename(path))
        else:
            fp = os.path.join(ROOT, os.path.normpath(path).lstrip("/"))
        fp = os.path.realpath(fp)
        if not os.path.isfile(fp):
            return self.send_plain(404)
        enc = None
        fp_send = fp
        ratio = 1.0
        for cand in (fp + ".br", os.path.join(HERE, "pkg", os.path.basename(fp) + ".br")):
            if os.path.isfile(cand):
                if "br" in self.headers.get("Accept-Encoding", ""):
                    enc, fp_send = "br", cand
                else:
                    # Safari sends no "br" over plain http. Pace as if it had, since a real
                    # deploy is https with brotli: the link time matches the brotli size.
                    ratio = os.path.getsize(cand) / max(1, os.path.getsize(fp))
                break
        with open(fp_send, "rb") as f:
            body = f.read()
        rtt, _ = PROFILES[state["profile"]]
        if rtt:
            time.sleep(rtt)
        self.send_response(200)
        self.send_header("Content-Type", TYPES.get(os.path.splitext(fp)[1], "application/octet-stream"))
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("Timing-Allow-Origin", "*")
        if enc:
            self.send_header("Content-Encoding", enc)
        self.end_headers()
        step = 16384
        for i in range(0, len(body), step):
            part = body[i : i + step]
            pace(len(part) * ratio)
            try:
                self.wfile.write(part)
            except (BrokenPipeError, ConnectionResetError):
                return


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8790
    srv = http.server.ThreadingHTTPServer(("127.0.0.1", port), H)
    srv.daemon_threads = True
    print("serving on 127.0.0.1:%d" % port, flush=True)
    srv.serve_forever()
