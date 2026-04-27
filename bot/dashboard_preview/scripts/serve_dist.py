#!/usr/bin/env python3
from __future__ import annotations

import argparse
import http.server
import socketserver
from pathlib import Path


class SpaRequestHandler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, directory: str, **kwargs):
        super().__init__(*args, directory=directory, **kwargs)

    def send_head(self):  # type: ignore[override]
        requested = self.translate_path(self.path)
        requested_path = Path(requested)

        if requested_path.exists() and requested_path.is_file():
            return super().send_head()

        self.path = "/index.html"
        return super().send_head()


def main() -> None:
    parser = argparse.ArgumentParser(description="Serve dashboard preview dist with SPA fallback.")
    parser.add_argument("--port", type=int, default=4174)
    parser.add_argument(
        "--dir",
        default=str(Path(__file__).resolve().parents[1] / "dist"),
        help="Directory to serve",
    )
    args = parser.parse_args()

    directory = Path(args.dir).resolve()
    if not directory.exists():
        raise SystemExit(f"Preview dist directory not found: {directory}")

    handler = lambda *a, **kw: SpaRequestHandler(*a, directory=str(directory), **kw)

    class ReusableTCPServer(socketserver.TCPServer):
        allow_reuse_address = True

    with ReusableTCPServer(("127.0.0.1", args.port), handler) as httpd:
        print(f"Dashboard preview available at http://127.0.0.1:{args.port}")
        httpd.serve_forever()


if __name__ == "__main__":
    main()
