import socket
import threading
import time
import tomllib
from pathlib import Path
from collections import defaultdict

SCRIPT_DIR = Path(__file__).resolve().parent
CANDIDATE_PATHS = [SCRIPT_DIR.parent / "test.toml", SCRIPT_DIR / "test.toml"]

counts = defaultdict(int)
counts_lock = threading.Lock()


def find_config():
    for path in CANDIDATE_PATHS:
        if path.is_file():
            return path
    tried = "\n  ".join(str(p) for p in CANDIDATE_PATHS)
    raise FileNotFoundError(f"Could not find test.toml. Tried:\n  {tried}")


def load_egress_ports():
    with open(find_config(), "rb") as f:
        config = tomllib.load(f)
    ports = []
    for instance in config["instances"]:
        for addr in instance["egress_addresses"]:
            host, port = addr.rsplit(":", 1)
            ports.append((instance["name"], int(port)))
    return ports


def run_listener(name, port):
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("127.0.0.1", port))
    s.listen()
    print(f"[listener:{name}:{port}] ready")

    while True:
        conn, addr = s.accept()
        with conn.makefile("rb") as f:
            data = f.read()
        conn.close()

        with counts_lock:
            counts[(name, port)] += 1

        preview = data[:40].decode(errors="replace")
        suffix = "..." if len(data) > 40 else ""


def print_summary(interval=5):
    while True:
        time.sleep(interval)
        dump_summary("summary (received so far)")


def dump_summary(title):
    with counts_lock:
        if not counts:
            return
        print(f"\n--- {title} ---")
        total = 0
        for (name, port), c in sorted(counts.items()):
            print(f"  {name} (:{port}): {c} messages")
            total += c
        print(f"  TOTAL: {total} messages")
        print("-" * (len(title) + 8) + "\n")


def main():
    ports = load_egress_ports()
    if not ports:
        print("No egress_addresses found in test.toml")
        return

    for name, port in ports:
        threading.Thread(target=run_listener, args=(name, port), daemon=True).start()

    threading.Thread(target=print_summary, daemon=True).start()

    try:
        threading.Event().wait()
    except KeyboardInterrupt:
        dump_summary("FINAL summary (received)")


if __name__ == "__main__":
    main()