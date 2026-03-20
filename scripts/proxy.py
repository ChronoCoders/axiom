import http.server
import urllib.request
import json
import hashlib
import secrets
import sys
import os

UPSTREAM = "http://127.0.0.1:8081"
WEB_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "web")

CONSOLE_USER = os.environ.get("AXIOM_CONSOLE_USER", "operator")
CONSOLE_PASS = os.environ.get("AXIOM_CONSOLE_PASS", "axiom")

active_tokens = set()

class ProxyHandler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=WEB_DIR, **kwargs)

    def do_GET(self):
        if self.path.startswith("/api/") or self.path.startswith("/health/") or self.path == "/validators" or self.path.startswith("/validators/"):
            self._proxy()
        else:
            super().do_GET()

    def do_POST(self):
        if self.path == "/auth/login":
            self._handle_login()
        elif self.path == "/auth/verify":
            self._handle_verify()
        elif self.path == "/auth/logout":
            self._handle_logout()
        else:
            self._proxy()

    def _handle_login(self):
        try:
            length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(length) if length > 0 else b"{}"
            data = json.loads(body)
            username = data.get("username", "")
            password = data.get("password", "")

            if username == CONSOLE_USER and password == CONSOLE_PASS:
                token = secrets.token_hex(32)
                active_tokens.add(token)
                self._json_response(200, {"token": token})
            else:
                self._json_response(401, {"error": "Invalid credentials"})
        except Exception:
            self._json_response(400, {"error": "Bad request"})

    def _handle_verify(self):
        try:
            length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(length) if length > 0 else b"{}"
            data = json.loads(body)
            token = data.get("token", "")
            if token in active_tokens:
                self._json_response(200, {"valid": True})
            else:
                self._json_response(401, {"valid": False})
        except Exception:
            self._json_response(400, {"error": "Bad request"})

    def _handle_logout(self):
        try:
            length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(length) if length > 0 else b"{}"
            data = json.loads(body)
            token = data.get("token", "")
            active_tokens.discard(token)
            self._json_response(200, {"ok": True})
        except Exception:
            self._json_response(400, {"error": "Bad request"})

    def _json_response(self, code, obj):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-cache")
        self.end_headers()
        self.wfile.write(body)

    def _proxy(self):
        url = UPSTREAM + self.path
        try:
            length = int(self.headers.get("Content-Length", 0))
            body = self.rfile.read(length) if length > 0 else None
            req = urllib.request.Request(url, data=body, method=self.command)
            req.add_header("Content-Type", self.headers.get("Content-Type", "application/json"))
            with urllib.request.urlopen(req, timeout=5) as resp:
                data = resp.read()
                self.send_response(resp.status)
                for key in ("Content-Type", "Content-Length"):
                    val = resp.getheader(key)
                    if val:
                        self.send_header(key, val)
                self.send_header("Cache-Control", "no-cache")
                self.end_headers()
                self.wfile.write(data)
        except Exception as e:
            self.send_response(502)
            self.send_header("Content-Type", "text/plain")
            self.end_headers()
            self.wfile.write(f"Upstream unavailable: {e}".encode())

    def end_headers(self):
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()

if __name__ == "__main__":
    port = 5000
    server = http.server.HTTPServer(("0.0.0.0", port), ProxyHandler)
    print(f"Proxy on 0.0.0.0:{port} -> {UPSTREAM}", flush=True)
    print(f"Console login: {CONSOLE_USER} / {'*' * len(CONSOLE_PASS)}", flush=True)
    server.serve_forever()
