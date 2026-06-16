#!/usr/bin/env python3
"""Static file server with HTTP Range support.

Python's stock `http.server` answers Range requests with `200 OK` and the whole
file, so seeking in an <audio> element restarts playback from 0. This serves
`206 Partial Content` for Range requests so the browser can seek into the WAVs.

    python3 webui/serve.py [port]   # default 8080
"""

import http.server
import os
import re
import sys


class RangeHandler(http.server.SimpleHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def copyfile(self, source, outputfile):
        rng = getattr(self, "_range", None)
        if rng is None:
            return super().copyfile(source, outputfile)
        start, length = rng
        source.seek(start)
        remaining = length
        while remaining > 0:
            chunk = source.read(min(64 * 1024, remaining))
            if not chunk:
                break
            outputfile.write(chunk)
            remaining -= len(chunk)

    def send_head(self):
        self._range = None
        rng = self.headers.get("Range")
        m = re.match(r"bytes=(\d+)-(\d*)$", rng) if rng else None
        if not m:
            return super().send_head()

        path = self.translate_path(self.path)
        try:
            f = open(path, "rb")
        except OSError:
            self.send_error(404)
            return None
        size = os.fstat(f.fileno()).st_size
        start = int(m.group(1))
        end = int(m.group(2)) if m.group(2) else size - 1
        end = min(end, size - 1)
        if start >= size:
            self.send_error(416)
            f.close()
            return None

        length = end - start + 1
        self.send_response(206)
        self.send_header("Content-Type", self.guess_type(path))
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        self.send_header("Content-Length", str(length))
        self.end_headers()
        self._range = (start, length)
        return f


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8080
    os.chdir(os.path.dirname(os.path.abspath(__file__)))
    print(f"serving webui with Range support on http://localhost:{port}")
    http.server.HTTPServer(("", port), RangeHandler).serve_forever()
