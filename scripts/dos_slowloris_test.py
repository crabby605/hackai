#!/usr/bin/env python3
"""Bounded slowloris / connection-exhaustion test against http://127.0.0.1:3000.
Opens N connections, sends partial headers slowly, holds them, then closes
and verifies the server recovers. Only against localhost, bounded in time.
"""
import http.client
import time

HOST = "127.0.0.1"
PORT = 3000
CONNS = 300   # connections to hold open
HOLD_SEC = 8  # how long to hold them


def main():
    conns = []
    held = 0
    try:
        for i in range(CONNS):
            try:
                c = http.client.HTTPConnection(HOST, PORT, timeout=5)
                c.connect()
                c.send(b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\n")
                time.sleep(0.01)
                c.send(b"X-Slow: ")
                time.sleep(0.01)
                c.send(f"{i}\r\n\r\n".encode())  # complete request, keep open
                conns.append(c)
                held += 1
            except Exception:
                break
        print(f"Held {held}/{CONNS} slow connections open for {HOLD_SEC}s")
        time.sleep(HOLD_SEC)

        # While holding: can new requests still get through?
        t0 = time.time()
        try:
            probe = http.client.HTTPConnection(HOST, PORT, timeout=5)
            probe.request("GET", "/healthz")
            resp = probe.getresponse()
            resp.read()
            probe.close()
            probe_status = resp.status
            probe_ms = (time.time() - t0) * 1000
        except Exception as e:
            probe_status = None
            probe_ms = (time.time() - t0) * 1000
            print(f"  probe during hold: error {e}")

        print(f"  probe during hold: {probe_status} in {probe_ms:.0f} ms")

    finally:
        for c in conns:
            try:
                c.close()
            except Exception:
                pass
        print(f"Closed all {held} held connections")

    # Recovery: does the server respond normally after the hold ends?
    time.sleep(1)
    ok = True
    for i in range(5):
        c = http.client.HTTPConnection(HOST, PORT, timeout=5)
        try:
            c.request("GET", "/healthz")
            resp = c.getresponse()
            resp.read()
            if resp.status != 200:
                ok = False
                print(f"  recovery request {i+1}: {resp.status}")
        except Exception as e:
            ok = False
            print(f"  recovery request {i+1}: error {e}")
        finally:
            try:
                c.close()
            except Exception:
                pass
    print("Recovery:", "OK" if ok else "FAILED")


if __name__ == "__main__":
    main()
