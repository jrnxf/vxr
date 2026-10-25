"""Static server for the test 3 browser builds, bound to 127.0.0.1.

python3 serve.py <port> <scene.vox>   serves web/, the fonts at /fetched/ and the scene at /scene.vox.
"""
import os
import sys
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer

PORT = int(sys.argv[1])
SCENE = os.path.abspath(sys.argv[2])
WEB = os.path.dirname(os.path.abspath(__file__))
FETCHED = os.path.join(os.path.dirname(WEB), "fetched")


class Handler(SimpleHTTPRequestHandler):
    extensions_map = {**SimpleHTTPRequestHandler.extensions_map, ".wasm": "application/wasm", ".js": "text/javascript"}

    def __init__(self, *a, **k):
        super().__init__(*a, directory=WEB, **k)

    def translate_path(self, path):
        p = path.split("?", 1)[0]
        if p == "/scene.vox":
            return SCENE
        if p.startswith("/fetched/"):
            return os.path.join(FETCHED, os.path.basename(p))
        return super().translate_path(path)

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def log_message(self, *a):
        pass


ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
