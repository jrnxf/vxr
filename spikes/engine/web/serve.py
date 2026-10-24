"""Static server for the browser bench, bound to 127.0.0.1. POST /results?name=x saves x.json.

python3 serve.py <port> <results dir> <vox file>...   (each .vox served at /vox/<file name>)
"""
import json
import os
import sys
import urllib.parse
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1])
RESULTS = os.path.abspath(sys.argv[2])
VOX = {os.path.basename(p): os.path.abspath(p) for p in sys.argv[3:]}
ROOT = os.path.dirname(os.path.abspath(__file__))


class Handler(SimpleHTTPRequestHandler):
    extensions_map = {**SimpleHTTPRequestHandler.extensions_map, ".wasm": "application/wasm", ".js": "text/javascript"}

    def __init__(self, *a, **k):
        super().__init__(*a, directory=ROOT, **k)

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def do_GET(self):
        path = urllib.parse.urlparse(self.path).path
        if path.startswith("/vox/"):
            f = VOX.get(path[5:])
            if not f:
                self.send_error(404)
                return
            data = open(f, "rb").read()
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            return
        super().do_GET()

    def do_POST(self):
        u = urllib.parse.urlparse(self.path)
        if u.path != "/results":
            self.send_error(404)
            return
        name = urllib.parse.parse_qs(u.query).get("name", ["unnamed"])[0]
        name = "".join(c for c in name if c.isalnum() or c in "-_.")
        body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        try:
            obj = json.loads(body)
        except ValueError:
            obj = {"raw": body.decode("utf-8", "replace")}
        with open(os.path.join(RESULTS, name + ".json"), "w") as fh:
            json.dump(obj, fh, indent=2)
        self.send_response(204)
        self.end_headers()

    def log_message(self, *a):
        pass


ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
