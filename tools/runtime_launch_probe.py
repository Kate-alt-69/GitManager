#!/usr/bin/env python3
"""Launch the Linux Tauri UI directly while satisfying its crash-helper heartbeat contract."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import secrets
import socketserver
import subprocess
import sys
import threading
import time


class HeartbeatHandler(socketserver.BaseRequestHandler):
    def handle(self) -> None:
        payload = bytearray()
        while len(payload) <= 8192:
            chunk = self.request.recv(4096)
            if not chunk:
                break
            payload.extend(chunk)
        try:
            message = json.loads(payload.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError):
            return

        server = self.server
        if message.get("token") != server.token:
            return
        if message.get("kind") == "heartbeat":
            with server.lock:
                server.heartbeats += 1
        elif message.get("kind") == "shutdown":
            with server.lock:
                server.shutdown_messages += 1


class HeartbeatServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True

    def __init__(self, token: str):
        self.token = token
        self.heartbeats = 0
        self.shutdown_messages = 0
        self.lock = threading.Lock()
        super().__init__(("127.0.0.1", 0), HeartbeatHandler)


def process_children(pid: int) -> list[int]:
    children_file = Path(f"/proc/{pid}/task/{pid}/children")
    try:
        return [int(value) for value in children_file.read_text(encoding="ascii").split()]
    except (OSError, ValueError):
        return []


def process_tree_rss_kb(root_pid: int) -> int:
    pending = [root_pid]
    seen: set[int] = set()
    total = 0
    while pending:
        pid = pending.pop()
        if pid in seen:
            continue
        seen.add(pid)
        status = Path(f"/proc/{pid}/status")
        try:
            for line in status.read_text(encoding="ascii", errors="replace").splitlines():
                if line.startswith("VmRSS:"):
                    total += int(line.split()[1])
                    break
        except (OSError, ValueError, IndexError):
            pass
        pending.extend(child for child in process_children(pid) if child not in seen)
    return total


def tail(path: Path, lines: int = 80) -> str:
    try:
        return "\n".join(path.read_text(encoding="utf-8", errors="replace").splitlines()[-lines:])
    except OSError:
        return "(application log unavailable)"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", required=True, type=Path)
    parser.add_argument("--duration", type=int, default=45)
    parser.add_argument("--log", required=True, type=Path)
    args = parser.parse_args()

    app = args.app.resolve()
    if not app.is_file():
        print(f"Application executable does not exist: {app}", file=sys.stderr)
        return 2
    if args.duration < 10:
        print("The runtime observation window must be at least 10 seconds.", file=sys.stderr)
        return 2

    args.log.parent.mkdir(parents=True, exist_ok=True)
    token = secrets.token_urlsafe(32)
    server = HeartbeatServer(token)
    address = f"127.0.0.1:{server.server_address[1]}"
    server_thread = threading.Thread(target=server.serve_forever, daemon=True)
    server_thread.start()

    environment = os.environ.copy()
    environment["GITMANAGER_CRASH_HELPER_ADDRESS"] = address
    environment["GITMANAGER_CRASH_HELPER_TOKEN"] = token

    process = None
    peak_rss_kb = 0
    try:
        with args.log.open("w", encoding="utf-8") as application_log:
            process = subprocess.Popen(
                [str(app), "--crash-helper-child"],
                env=environment,
                stdout=application_log,
                stderr=subprocess.STDOUT,
            )
            print(f"Started GitManager UI (pid={process.pid}); observing for {args.duration}s.")
            deadline = time.monotonic() + args.duration
            while time.monotonic() < deadline:
                exit_code = process.poll()
                if exit_code is not None:
                    print(f"GitManager exited early with code {exit_code}.")
                    print(tail(args.log))
                    return 1
                peak_rss_kb = max(peak_rss_kb, process_tree_rss_kb(process.pid))
                time.sleep(0.25)

        with server.lock:
            heartbeat_count = server.heartbeats
        if heartbeat_count < 1:
            print("The UI stayed alive but never reported a frontend heartbeat.")
            print(tail(args.log))
            return 1

        print(f"GitManager remained alive for {args.duration}s and reported {heartbeat_count} UI heartbeat(s).")
        if peak_rss_kb <= 0:
            print("Could not read GitManager's process-tree RSS from /proc.", file=sys.stderr)
            print(tail(args.log))
            return 1
        print(f"GITMANAGER_PEAK_RSS_KB={peak_rss_kb}")
        process.terminate()
        try:
            process.wait(timeout=8)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
        return 0
    except OSError as error:
        print(f"Could not launch GitManager: {error}", file=sys.stderr)
        if process and process.poll() is None:
            process.kill()
            process.wait()
        return 1
    finally:
        server.shutdown()
        server.server_close()
        server_thread.join(timeout=2)


if __name__ == "__main__":
    raise SystemExit(main())
