import socket
import threading
import time
import random
import string
import tomllib
from pathlib import Path
from collections import defaultdict

SCRIPT_DIR = Path(__file__).resolve().parent
CANDIDATE_PATHS = [SCRIPT_DIR.parent / "test.toml", SCRIPT_DIR / "test.toml"]

sent_counts = defaultdict(int)
failed_counts = defaultdict(int)
counts_lock = threading.Lock()


def find_config():
    for path in CANDIDATE_PATHS:
        if path.is_file():
            return path
    tried = "\n  ".join(str(p) for p in CANDIDATE_PATHS)
    raise FileNotFoundError(f"Could not find test.toml. Tried:\n  {tried}")


def load_ingress_ports():
    with open(find_config(), "rb") as f:
        config = tomllib.load(f)
    ports = []
    for instance in config["instances"]:
        for addr in instance["ingress_addresses"]:
            host, port = addr.rsplit(":", 1)
            ports.append((instance["name"], int(port)))
    return ports


def run_sender(name, port, label, min_delay=0.0001, max_delay=0.01):
    seq = 0
    while True:
        time.sleep(random.uniform(min_delay, max_delay))
        seq += 1
        size = random.randint(50, 2000)
        payload = f"{label}-{seq}-".encode() + bytes(random.getrandbits(8) for _ in range(size))
        try:
            s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
            s.settimeout(2)
            s.connect(("127.0.0.1", port))
            s.sendall(payload)
            s.close()
            with counts_lock:
                sent_counts[(name, port)] += 1
        except (ConnectionRefusedError, socket.timeout, OSError):
            with counts_lock:
                failed_counts[(name, port)] += 1


def print_summary(interval=5):
    while True:
        time.sleep(interval)
        dump_summary("summary (sent so far)")


def dump_summary(title):
    with counts_lock:
        keys = sorted(set(sent_counts) | set(failed_counts))
        if not keys:
            return
        print(f"\n--- {title} ---")
        total_sent = total_failed = 0
        for name, port in keys:
            s, f = sent_counts[(name, port)], failed_counts[(name, port)]
            print(f"  {name} (:{port}): sent={s} failed={f}")
            total_sent += s
            total_failed += f
        print(f"  TOTAL: sent={total_sent} failed={total_failed}")
        print("-" * (len(title) + 8) + "\n")


def main():
    ports = load_ingress_ports()
    if not ports:
        print("No ingress_addresses found in test.toml")
        return

    labels = string.ascii_lowercase
    for i, (name, port) in enumerate(ports):
        label = labels[i % len(labels)]
        threading.Thread(target=run_sender, args=(name, port, label), daemon=True).start()

    threading.Thread(target=print_summary, daemon=True).start()

    try:
        threading.Event().wait()
    except KeyboardInterrupt:
        dump_summary("FINAL summary (sent)")


if __name__ == "__main__":
    main()