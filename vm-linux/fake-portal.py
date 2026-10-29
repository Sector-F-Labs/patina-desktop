#!/usr/bin/env python3
"""A fake captive portal for the Linux test VM, on the fake access point's address.

NetworkManager's connectivity check (patina-vm.conf) asks this for a page. While the
marker file exists it answers as a café's gateway does, with a redirect to a login
page, whose button removes the marker; without it, the answer NetworkManager takes as
being online. `fake-hardware.sh portal on|off` makes or removes the marker."""
import http.server
import os

MARKER = "/run/patina-fake-portal"
PAGE = b"""<!doctype html><title>Cafe Wi-Fi</title><h1>Cafe Wi-Fi</h1>
<form method=post action=/accept><button>Accept the terms and connect</button></form>"""


class Portal(http.server.BaseHTTPRequestHandler):
    def reply(self, status, headers, body=b""):
        self.send_response(status)
        for name, value in headers.items():
            self.send_header(name, value)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        if not os.path.exists(MARKER):
            self.reply(200, {"X-NetworkManager-Status": "online"}, b"NetworkManager is online\n")
        elif self.path == "/login":
            self.reply(200, {"Content-Type": "text/html"}, PAGE)
        else:
            self.reply(302, {"Location": "http://10.42.0.1/login"})

    def do_POST(self):
        if self.path == "/accept" and os.path.exists(MARKER):
            os.remove(MARKER)
        self.reply(200, {"Content-Type": "text/html"}, b"<h1>You are online.</h1>")

    def log_message(self, *_):
        pass


http.server.ThreadingHTTPServer(("10.42.0.1", 80), Portal).serve_forever()
