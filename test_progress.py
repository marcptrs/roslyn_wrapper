#!/usr/bin/env python3
"""
Test script to capture progress notifications from Roslyn via the wrapper.
This script loads a large project and monitors for progress-related LSP messages.

Usage: python test_progress.py /path/to/project/with/solution.sln
"""

import json
import os
import subprocess
import sys
import time
from pathlib import Path

_id_counter = 0


def next_id():
    global _id_counter
    _id_counter += 1
    return _id_counter


def launch_wrapper(wrapper_path, log_file):
    """Launch the roslyn-wrapper binary"""
    proc = subprocess.Popen(
        [str(wrapper_path)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
        env={**os.environ, "ROSLYN_WRAPPER_LOG": str(log_file)},
    )
    return proc


def send(proc, msg):
    """Send LSP message to the process"""
    data = json.dumps(msg)
    header = f"Content-Length: {len(data)}\r\n\r\n"
    message = header + data
    method_or_id = msg.get("method") or f"id={msg.get('id')}"
    print(f">>> SENDING: {method_or_id} {len(data)} bytes")
    proc.stdin.write(message)
    proc.stdin.flush()


def read_message(proc, timeout=2.0):
    """Read LSP message from process stdout with timeout"""
    import select

    # Check if data is available
    ready, _, _ = select.select([proc.stdout], [], [], timeout)
    if not ready:
        return None

    # Read headers
    headers = {}
    while True:
        line = proc.stdout.readline()
        if not line:
            return None
        line = line.rstrip("\r\n")
        if line == "":
            break
        if ":" not in line:
            continue
        k, v = line.split(":", 1)
        headers[k.strip().lower()] = v.strip()

    # Read body
    length = int(headers.get("content-length", 0))
    if length == 0:
        return None

    body = proc.stdout.read(length)
    msg = json.loads(body) if body else None

    # Log interesting messages
    if msg:
        method = msg.get("method", "")
        msg_id = msg.get("id", "")

        if "progress" in method.lower():
            print("<<< PROGRESS MESSAGE:")
            print(json.dumps(msg, indent=2))
        elif method == "window/workDoneProgress/create":
            print("<<< CREATE PROGRESS TOKEN:")
            print(json.dumps(msg, indent=2))
        elif method == "window/logMessage":
            # Print log messages content to see if they contain progress info
            log_msg = msg.get("params", {}).get("message", "")
            log_type = msg.get("params", {}).get("type", 0)
            # Type: 1=Error, 2=Warning, 3=Info, 4=Log
            print(f"<<< LOG ({log_type}): {log_msg}")
        elif method:
            print(f"<<< NOTIFICATION: {method}")
        elif msg_id:
            print(f"<<< RESPONSE: id={msg_id}")

    return msg


def main():
    if len(sys.argv) != 2:
        print("Usage: test_progress.py /path/to/solution.sln")
        sys.exit(1)

    solution_path = Path(sys.argv[1]).resolve()
    if not solution_path.exists():
        sys.exit(f"Solution not found: {solution_path}")

    project_root = solution_path.parent

    # Find wrapper binary
    wrapper_path = Path(__file__).parent / "target" / "release" / "roslyn-wrapper"
    if not wrapper_path.exists():
        sys.exit(
            f"Wrapper not found at {wrapper_path}. Run 'cargo build --release' first."
        )

    log_file = Path(__file__).parent / "test_progress.log"
    print(f"Wrapper: {wrapper_path}")
    print(f"Solution: {solution_path}")
    print(f"Log file: {log_file}")
    print(f"Project root: {project_root}")
    print()

    # Launch wrapper
    print("Launching wrapper...")
    proc = launch_wrapper(wrapper_path, log_file)
    time.sleep(0.5)  # Give it a moment to start

    # Send initialize
    print("\nSending initialize request...")
    initialize = {
        "jsonrpc": "2.0",
        "id": next_id(),
        "method": "initialize",
        "params": {
            "processId": os.getpid(),
            "rootUri": project_root.as_uri(),
            "capabilities": {
                "window": {
                    "workDoneProgress": True,
                },
                "textDocument": {
                    "publishDiagnostics": {},
                },
                "workspace": {
                    "workspaceFolders": True,
                    "configuration": True,
                },
            },
            "initializationOptions": {
                "solution": solution_path.as_uri(),
            },
            "workspaceFolders": [
                {"uri": project_root.as_uri(), "name": project_root.name}
            ],
        },
    }
    send(proc, initialize)

    # Wait for initialize response
    print("\nWaiting for initialize response...")
    init_resp = None
    for _ in range(30):  # Try for up to 60 seconds
        msg = read_message(proc)
        if msg and msg.get("id") == 1:
            init_resp = msg
            print(f"<<< INITIALIZE RESPONSE received")
            break

    if not init_resp:
        sys.exit("No initialize response received")

    # Send initialized notification
    print("\nSending initialized notification...")
    send(proc, {"jsonrpc": "2.0", "method": "initialized", "params": {}})

    # Keep reading messages for a while to capture progress notifications
    print("\n" + "=" * 80)
    print("MONITORING FOR PROGRESS NOTIFICATIONS (60 seconds)...")
    print("=" * 80 + "\n")

    start_time = time.time()
    message_count = 0
    progress_messages = []

    while time.time() - start_time < 60:  # Monitor for 60 seconds
        msg = read_message(proc, timeout=1.0)
        if msg:
            message_count += 1
            method = msg.get("method", "")

            # Capture progress-related messages
            if (
                "progress" in method.lower()
                or method == "window/workDoneProgress/create"
            ):
                progress_messages.append(msg)

    # Summary
    print("\n" + "=" * 80)
    print("SUMMARY")
    print("=" * 80)
    print(f"Total messages received: {message_count}")
    print(f"Progress-related messages: {len(progress_messages)}")

    if progress_messages:
        print("\nProgress messages captured:")
        for i, msg in enumerate(progress_messages, 1):
            method_name = msg.get("method", "response")
            print(f"\n{i}. {method_name}:")
            print("   " + json.dumps(msg, indent=2))
    else:
        print("\n⚠️  NO PROGRESS MESSAGES DETECTED")
        print(
            "   This means Roslyn is not sending $/progress or window/workDoneProgress/create"
        )

    print(f"\nCheck wrapper log at: {log_file}")

    # Shutdown
    print("\nShutting down...")
    send(proc, {"jsonrpc": "2.0", "id": next_id(), "method": "shutdown", "params": {}})
    time.sleep(0.5)
    send(proc, {"jsonrpc": "2.0", "method": "exit", "params": {}})
    time.sleep(0.5)
    proc.terminate()

    print("Done!")


if __name__ == "__main__":
    main()
