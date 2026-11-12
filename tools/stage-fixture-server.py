#!/usr/bin/env python3
"""Stage-output fixture server for the live-web proof.

Serves one page at `/` that renders like a browser-source stage overlay and
switches on the hash route, exactly the shape the conductor's pass bar names:

    http://localhost:4000/#/output1

Only loopback is bound (127.0.0.1) — nothing leaves the machine.

Usage: python3 tools/stage-fixture-server.py [--port 4000]
"""
from __future__ import annotations

import argparse
import http.server
import socketserver

PAGE = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Stage output</title>
<style>
  /* Transparent overlay: the room sees the projector's own background
     through this page; only the verse text paints. */
  /* No scrollbars: the overlay must never show chrome of any kind. */
  html, body { margin: 0; height: 100%; overflow: hidden; background: transparent;
               color: #fff; font-family: Verdana, "DejaVu Sans", sans-serif; }
  #out { height: 100%; display: flex; flex-direction: column;
         align-items: center; justify-content: center; gap: 28px; padding: 40px; }
  .verse { font-size: 58px; font-weight: 700; line-height: 1.25; text-align: center; }
  .ref { font-size: 20px; letter-spacing: .08em; opacity: .75; text-transform: uppercase; }
  #route { position: fixed; right: 14px; top: 10px; font-size: 12px; opacity: .45; }
</style>
</head>
<body>
<div id="out"></div>
<div id="route"></div>
<script>
  // Hash routes are the stage protocol: #/output1 = the projected lyrics.
  const view = () => {
    const hash = location.hash || "#/";
    document.getElementById("route").textContent = hash;
    const out = document.getElementById("out");
    if (hash.startsWith("#/output1")) {
      out.innerHTML =
        '<div class="verse">Ye are the light of the world.</div>' +
        '<div class="verse">A city that is set on an hill cannot be hid.</div>' +
        '<div class="ref">Matthew 5:14</div>';
    } else {
      out.innerHTML = '<div class="verse">Stage server</div>' +
        '<div class="ref">open #/output1</div>';
    }
  };
  window.addEventListener("hashchange", view);
  view();
</script>
</body>
</html>
"""


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self) -> None:  # noqa: N802 (stdlib naming)
        body = PAGE.encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args) -> None:  # quiet: fixture, not a server app
        return


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=4000)
    args = ap.parse_args()
    socketserver.TCPServer.allow_reuse_address = True
    with socketserver.TCPServer(("127.0.0.1", args.port), Handler) as httpd:
        print(f"stage fixture on http://localhost:{args.port}/#/output1", flush=True)
        httpd.serve_forever()


if __name__ == "__main__":
    main()
