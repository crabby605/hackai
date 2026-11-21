#!/usr/bin/env python3
"""Malformed / oversized request robustness test against http://127.0.0.1:3000.
Checks the server never returns 500 or crashes on hostile input. Stdlib only.
"""
import http.client
import json

HOST = "127.0.0.1"
PORT = 3000
PATH = "/api/hackatime/v1/users/current/heartbeats.bulk"


def raw_request(method, path, body=b"", extra_headers=None, timeout=5):
    """Send a raw request, returning (status, reason)."""
    c = http.client.HTTPConnection(HOST, PORT, timeout=timeout)
    headers = {"Content-Type": "application/json", "Connection": "close"}
    if extra_headers:
        headers.update(extra_headers)
    try:
        c.request(method, path, body=body, headers=headers)
        resp = c.getresponse()
        resp.read()
        return resp.status, resp.reason
    except Exception as e:
        return None, repr(e)
    finally:
        try:
            c.close()
        except Exception:
            pass


def main():
    tests = [
        # Oversized body (>256KB)
        ("oversized 300KB", "POST", PATH, b"x" * (300 * 1024)),
        # Invalid JSON
        ("invalid JSON", "POST", PATH, b"{not valid json"),
        # Truncated JSON
        ("truncated JSON", "POST", PATH, b'[{"id":"a","time":1.0'),
        # Valid JSON but wrong shape (string instead of array)
        ("JSON string not array", "POST", PATH, b'"just a string"'),
        # Empty body
        ("empty body", "POST", PATH, b""),
        # No Content-Type header
        ("no content-type", "POST", PATH, b'[{"id":"a","time":1.0}]',
         {"Content-Type": "text/plain"}),
        # Garbage bytes
        ("garbage bytes", "POST", PATH, b"\x00\x01\x02\xff\xfe\xfd garbage"),
        # Deeply nested JSON
        ("deeply nested", "POST", PATH, b"[" * 10000 + b"]" * 10000),
        # Huge single key
        ("huge key", "POST", PATH, json.dumps({"a" * 100000: 1}).encode()),
        # Very long URL path
        ("long path", "GET", "/" + "a" * 8000, b""),
        # Duplicate Content-Length (smuggling-ish)
        ("dup content-length", "POST", PATH, b"{}",
         {"Content-Type": "application/json", "Content-Length": "2",
          "Content-Length": "100"}),
    ]

    all_ok = True
    for name, method, path, body, *rest in tests:
        extra = rest[0] if rest else None
        status, reason = raw_request(method, path, body, extra)
        ok = status is not None and 400 <= status < 500
        if status == 500 or status is None:
            all_ok = False
        print(("PASS" if ok else "FAIL"),
              f"{name}: {method} {path[:40]} -> {status} {reason}")

    # Post-test health check: server still alive?
    status, _ = raw_request("GET", "/healthz")
    alive = status == 200
    print(("PASS" if alive else "FAIL"), f"post-test /healthz -> {status} (server alive)")
    if not alive:
        all_ok = False

    print()
    print("Malformed-request test", "PASSED" if all_ok else "FAILED")


if __name__ == "__main__":
    main()
