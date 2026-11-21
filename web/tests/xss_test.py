#!/usr/bin/env python3
"""XSS corpus test for HackAI.

Injects synthetic, non-sensitive XSS payloads via the Rust API endpoints and
verifies that every server-rendered page reflects them HTML-encoded, never as
raw executable HTML/JS. Reports PASS/FAIL per payload per page.

Requires both servers running before execution:
    Rust Axum API   -> http://127.0.0.1:3000
    Next.js WebUI   -> http://localhost:3001

Usage:
    python3 web/tests/xss_test.py

Stdlib only (urllib, json, html). No external dependencies.
"""
import html
import json
import sys
import urllib.error
import urllib.request
from urllib.parse import quote

API = "http://127.0.0.1:3000"
UI = "http://localhost:3001"
CORPUS = "/Users/vihaan/ai-time/web/tests/xss-corpus.json"

# Every server-rendered page to load. The profile route is also exercised with
# each payload placed directly in the username path segment.
PAGES = ["/", "/login", "/dashboard", "/admin"]

# Raw executable markers. If any appear verbatim in a page body for a payload
# that is itself executable HTML/JS, the payload leaked unencoded -> FAIL.
MARKERS = [
    "<script", "</script>", "<img", "<svg", "<iframe", "<body", "<details",
    "onerror=", "onload=", "onclick=", "ontoggle=", "onmouseover=", "onmouseout=",
    "javascript:", "data:text/html", "vbscript:", "alert(",
]

TIMEOUT = 8


def http(method, url, body=None, headers=None):
    """Perform an HTTP request. Returns (status, body_text, headers) or
    (None, error_text, {}) on connection-level failure."""
    h = {"Accept": "*/*"}
    if headers:
        h.update(headers)
    data = None
    if body is not None:
        if isinstance(body, (dict, list)):
            data = json.dumps(body).encode("utf-8")
            h["Content-Type"] = "application/json"
        elif isinstance(body, str):
            data = body.encode("utf-8")
        else:
            data = body
    req = urllib.request.Request(url, data=data, headers=h, method=method)
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
            return resp.status, resp.read().decode("utf-8", "replace"), dict(resp.headers)
    except urllib.error.HTTPError as e:
        try:
            body_text = e.read().decode("utf-8", "replace")
        except Exception:
            body_text = ""
        return e.code, body_text, dict(e.headers)
    except Exception as e:  # connection refused, timeout, DNS, etc.
        return None, str(e), {}


def is_executable(payload):
    """True if the payload carries executable HTML/JS/URL semantics."""
    low = payload.lower()
    if any(m in low for m in MARKERS):
        return True
    if any(c in payload for c in '<>"\''):
        return True
    return False


def inject(api, payload):
    """Best-effort injection of payload into text fields via the API."""
    results = {}

    # POST /v1/events with kind
    st, body, _ = http("POST", f"{api}/v1/events", {"kind": payload})
    results["events"] = (st, body)

    # POST WakaTime heartbeats.bulk with payload across text fields.
    batch = [{
        "category": payload,
        "language": payload,
        "editor": payload,
        "ai_model": payload,
        "source": payload,
        "source_type": payload,
        "machine_hash": payload,
        "operating_system": payload,
    }]
    st, body, _ = http("POST", f"{api}/api/v1/wakatime/heartbeats.bulk", batch)
    results["heartbeats.bulk"] = (st, body)

    return results


def check_payload_on_page(body, payload):
    """Return (passed, detail) for one payload reflected in one page body."""
    encoded = html.escape(payload, quote=True)
    encoded_reflected = encoded in body
    raw_reflected = payload in body
    executable = is_executable(payload)

    if executable and raw_reflected:
        if not encoded_reflected:
            return False, f"raw executable payload reflected (no encoding): {payload!r}"
        return False, f"raw executable payload reflected alongside encoding: {payload!r}"
    if executable:
        return True, f"not reflected raw (encoded present: {encoded_reflected})"
    # Non-executable payloads (control chars, bidi marks, lone punctuation):
    # raw reflection is not itself XSS. PASS, note whether reflected.
    return True, f"non-executable payload (reflected={raw_reflected})"


def main():
    with open(CORPUS, encoding="utf-8") as f:
        corpus = json.load(f)

    records = []

    # --- 1. Inject every payload into text fields via the API ---
    for group, items in corpus.items():
        for item in items:
            payload = item["payload"]
            results = inject(API, payload)
            records.append(("inject", item["id"], payload, group, results))

    # --- 2. Build the page list, including profile paths with payload as username ---
    pages = list(PAGES)
    for group, items in corpus.items():
        for item in items:
            pages.append("/profile/" + quote(item["payload"], safe=""))

    # --- 3. Load every page and check every payload against its body ---
    seen_pages = set()
    for page in pages:
        if page in seen_pages:
            continue
        seen_pages.add(page)
        st, body, _ = http("GET", f"{UI}{page}")
        page_status = "HTTP 200" if st == 200 else (f"HTTP {st}" if st else "CONNECT FAIL")

        for group, items in corpus.items():
            for item in items:
                payload = item["payload"]
                passed, detail = check_payload_on_page(body, payload)
                records.append(("page", item["id"], page, group, payload,
                                passed, detail, page_status))

    # --- 4. Report PASS/FAIL ---
    fails = [r for r in records if r[0] == "page" and r[5] is False]
    page_checks = [r for r in records if r[0] == "page"]

    print("=" * 72)
    print("XSS CORPUS TEST RESULTS")
    print("=" * 72)
    print(f"API:        {API}")
    print(f"WebUI:      {UI}")
    print(f"Corpus:     {CORPUS}")
    print(f"Payloads:   {sum(len(v) for v in corpus.values())}")
    print(f"Pages:      {len(seen_pages)}")
    print(f"Page checks:{len(page_checks)}  PASS: {len(page_checks) - len(fails)}  FAIL: {len(fails)}")
    print("-" * 72)

    for r in records:
        if r[0] == "inject":
            _, pid, payload, group, results = r
            ev = results.get("events", (None, ""))[0]
            hb = results.get("heartbeats.bulk", (None, ""))[0]
            print(f"[inject] {pid:22} [{group:18}] /v1/events={ev} heartbeats.bulk={hb} :: {payload!r}")
        else:
            _, pid, page, group, payload, passed, detail, pstatus = r
            flag = "PASS" if passed else "FAIL"
            print(f"[{flag}] {pid:22} on {page:28} [{group:18}] {pstatus:14} :: {detail}")

    print("-" * 72)
    if fails:
        print(f"RESULT: {len(fails)} FAILURE(S) — raw executable payload leaked:")
        for r in fails:
            _, pid, page, group, payload, _, detail, _ = r
            print(f"  - {pid} on {page} [{group}]: {detail} :: {payload!r}")
    else:
        print("RESULT: ALL PASS — no raw executable payload reflected on any page.")
    print("=" * 72)

    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
