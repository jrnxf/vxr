"""Static server for the test 2 browser bench, bound to 127.0.0.1.

python3 serve.py <port> <results dir>
GET serves this folder (no caching). POST /results?name=x saves the body as <results dir>/x.json,
POST /log?name=x appends a progress line to <results dir>/x.log.
"""
import os
import sys
import urllib.parse
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1])
RESULTS = os.path.abspath(sys.argv[2])
ROOT = os.path.dirname(os.path.abspath(__file__))


class Handler(SimpleHTTPRequestHandler):
    extensions_map = {**SimpleHTTPRequestHandler.extensions_map, ".wasm": "application/wasm", ".js": "text/javascript"}

    def __init__(self, *a, **k):
        super().__init__(*a, directory=ROOT, **k)

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def log_message(self, *a):
        pass

    def do_POST(self):
        u = urllib.parse.urlparse(self.path)
        name = urllib.parse.parse_qs(u.query).get("name", ["unnamed"])[0]
        name = "".join(c for c in name if c.isalnum() or c in "-_.")
        body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        os.makedirs(RESULTS, exist_ok=True)
        if u.path == "/results":
            with open(os.path.join(RESULTS, name + ".json"), "wb") as f:
                f.write(body)
        elif u.path == "/log":
            with open(os.path.join(RESULTS, name + ".log"), "ab") as f:
                f.write(body + b"\n")
        else:
            self.send_error(404)
            return
        self.send_response(204)
        self.end_headers()


ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
