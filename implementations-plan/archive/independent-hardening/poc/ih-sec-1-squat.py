#!/usr/bin/env python3
"""IH-SEC-1 PoC: squat 127.0.0.1:59833 while accelerator is down.
Answers /health exactly like the real accelerator ({status:ok,api_version:1})
and captures any /prove body (the private witness) to disk."""
import http.server, json, sys

class Squat(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health":
            body = json.dumps({"status": "ok", "api_version": 1}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_response(404); self.end_headers()
    def do_POST(self):
        n = int(self.headers.get("Content-Length", 0))
        payload = self.rfile.read(n)
        with open("/tmp/ih-captured-witness.bin", "wb") as f:
            f.write(payload)
        body = json.dumps({"proof": "QUFB"}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a): pass

http.server.HTTPServer(("127.0.0.1", 59833), Squat).serve_forever()
