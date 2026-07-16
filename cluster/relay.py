#!/usr/bin/env python3
"""Rhombic Strips relay: a tiny loopback HTTP server between the web page
and the `strip_stream` binary.

Started by `cluster/serve.sh`; not meant to be run by hand. It listens on
127.0.0.1 only. In cluster mode the browser reaches it through the user's
own `ssh -L` tunnel, so every job runs under that user's account and
allocation. A pairing token (printed by serve.sh, entered once in the page)
keeps other local users on a shared login node — and random web pages doing
drive-by requests to localhost — from submitting jobs.

Protocol (all responses send permissive CORS headers; the token is the gate):

  GET  /ping   -> {"ok":true,"mode":"slurm"|"direct","threads":n,"host":...,
                   "paired":bool}     paired = X-Rhombic-Token matched
  POST /job    -> requires X-Rhombic-Token; body is one strip_stream job:
                  {"graph":...,"cyclic":...,"mode":...,"cap":...}
                  Response streams NDJSON exactly as strip_stream emits it
                  (chunked); srun's stderr chatter is forwarded as
                  {"type":"note",...} lines. Aborting the fetch kills the
                  process group, which releases the Slurm allocation.

Python stdlib only, so it runs on any login node with python3.
"""

import json
import os
import shlex
import signal
import socket
import subprocess
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

TOKEN = os.environ.get("RHOMBIC_TOKEN", "")
BIN = os.environ.get("RHOMBIC_BIN", "strip_stream")
MODE = os.environ.get("RHOMBIC_MODE", "direct")  # "direct" | "slurm"
SRUN_ARGS = shlex.split(os.environ.get("RHOMBIC_SRUN_ARGS", ""))
PORT = int(os.environ.get("RHOMBIC_PORT", "8642"))
HOST = socket.gethostname().split(".")[0]
MAX_JOB_BYTES = 32 * 1024 * 1024

if not TOKEN:
    sys.exit("relay.py: RHOMBIC_TOKEN is not set (start me via serve.sh)")


def command():
    if MODE == "slurm":
        return ["srun", "--unbuffered", *SRUN_ARGS, BIN]
    return [BIN]


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):  # quiet; errors go through log_error
        pass

    def _paired(self):
        return TOKEN and self.headers.get("X-Rhombic-Token", "") == TOKEN

    def _cors(self):
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header(
            "Access-Control-Allow-Headers", "Content-Type, X-Rhombic-Token"
        )
        # Chrome Private Network Access: public https page -> loopback.
        self.send_header("Access-Control-Allow-Private-Network", "true")

    def _json(self, code, obj):
        body = json.dumps(obj).encode()
        self.send_response(code)
        self._cors()
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    # -- routes ---------------------------------------------------------------

    def do_OPTIONS(self):
        self.send_response(204)
        self._cors()
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        if self.path.split("?")[0] != "/ping":
            return self._json(404, {"error": "unknown path"})
        self._json(
            200,
            {
                "ok": True,
                "mode": MODE,
                "threads": os.cpu_count() or 1,
                "host": HOST,
                "paired": self._paired(),
            },
        )

    def do_POST(self):
        if self.path != "/job":
            return self._json(404, {"error": "unknown path"})
        if not self._paired():
            return self._json(401, {"error": "bad or missing pairing token"})

        length = int(self.headers.get("Content-Length", "0"))
        if not 0 < length <= MAX_JOB_BYTES:
            return self._json(400, {"error": "bad job size"})
        raw = self.rfile.read(length)
        try:
            json.loads(raw)  # reject garbage before burning an allocation
        except ValueError:
            return self._json(400, {"error": "job is not valid JSON"})

        try:
            proc = subprocess.Popen(
                command(),
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                start_new_session=True,  # its own process group, for killpg
            )
        except OSError as e:
            return self._json(500, {"error": f"cannot start {command()[0]}: {e}"})

        self.send_response(200)
        self._cors()
        self.send_header("Content-Type", "application/x-ndjson")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()

        lock = threading.Lock()  # stdout & stderr threads share the socket

        def chunk(line: bytes) -> bool:
            with lock:
                try:
                    self.wfile.write(b"%x\r\n%s\r\n" % (len(line), line))
                    self.wfile.flush()
                    return True
                except OSError:
                    return False  # browser aborted the fetch

        def kill():
            try:
                os.killpg(proc.pid, signal.SIGTERM)
            except (ProcessLookupError, PermissionError):
                pass

        def pump_stderr():
            # srun chatter ("job queued and waiting for resources", ...)
            # becomes note messages the page can show in its log line.
            for line in proc.stderr:
                text = line.decode("utf-8", "replace").strip()
                if not text:
                    continue
                note = json.dumps({"type": "note", "message": text}) + "\n"
                if not chunk(note.encode()):
                    kill()
                    return

        err_thread = threading.Thread(target=pump_stderr, daemon=True)
        err_thread.start()

        try:
            proc.stdin.write(raw + b"\n")
            proc.stdin.flush()
            proc.stdin.close()  # one-shot job; EOF is fine for strip_stream

            for line in proc.stdout:
                if not chunk(line):
                    kill()
                    break
        except (OSError, BrokenPipeError):
            kill()
        finally:
            proc.wait()
            err_thread.join(timeout=2)
            with lock:
                try:
                    self.wfile.write(b"0\r\n\r\n")  # end of chunked body
                except OSError:
                    pass
            self.close_connection = True


def main():
    server = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    mode = MODE if MODE != "slurm" else f"slurm ({' '.join(SRUN_ARGS) or 'site defaults'})"
    print(f"relay: listening on 127.0.0.1:{PORT}  [{mode}, bin={BIN}]", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
