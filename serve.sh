#!/bin/sh
# Serves web/ on port 8080, telling the browser not to cache anything, so a reload always picks up a fresh build.
cd "$(dirname "$0")"
echo "http://localhost:8080"
python3 -c '
import functools, http.server
class NoCache(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()
http.server.ThreadingHTTPServer(("", 8080), functools.partial(NoCache, directory="web")).serve_forever()
'
