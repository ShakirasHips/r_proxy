import subprocess

p1 = subprocess.Popen(["python", "testing/listen.py"])
p2 = subprocess.Popen(["python", "testing/send.py"])

try:
    p1.wait()
    p2.wait()
except KeyboardInterrupt:
    print("Interrupted, shutting down...")
finally:
    for p in (p1, p2):
        if p.poll() is None:  # still running
            p.terminate()
            try:
                p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                p.kill()