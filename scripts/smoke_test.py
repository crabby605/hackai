#!/usr/bin/env python3
"""Smoke test for the HackAI API (http://127.0.0.1:3000).
Exits non-zero if any expected endpoint misbehaves. Stdlib only.
"""
import sys
import urllib.request
import urllib.error

BASE = "http://127.0.0.1:3000"


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None  # don't follow redirects; surface the status as-is


def check(path, expect_statuses, method="GET", body=None):
    url = BASE + path
    data = None
    if body is not None:
        import json
        data = json.dumps(body).encode()
    req = urllib.request.Request(url, data=data, method=method,
                                 headers={"Content-Type": "application/json"})
    opener = urllib.request.build_opener(NoRedirect)
    try:
        with opener.open(req, timeout=5) as resp:
            status = resp.status
    except urllib.error.HTTPError as e:
        status = e.code
    except Exception as e:
        print(f"FAIL {method} {path}: connection error — {e}")
        return False
    ok = status in expect_statuses
    print(("PASS" if ok else "FAIL"), f"{method} {path} -> {status} (expect {expect_statuses})")
    return ok


def main():
    checks = [
        ("/healthz", {200}),
        ("/auth/login", {303}),          # OIDC configured -> redirect
        ("/api/v1/me", {401}),           # no session -> unauthorized
        ("/api/v1/dashboard/stats", {401}),
        ("/api/v1/users", {404}),        # no route without trailing context
        ("/nonexistent", {404}),
    ]
    failures = 0
    for path, expects in checks:
        if not check(path, expects):
            failures += 1
    print()
    if failures:
        print(f"Smoke test FAILED: {failures} check(s) failed")
        sys.exit(1)
    print("Smoke test PASSED: all checks OK")


if __name__ == "__main__":
    main()
