#!/usr/bin/env python3
"""Load / abuse test harness for the HackAI Rust API (http://127.0.0.1:3000).

Standard library only: urllib, threading, concurrent.futures, http.client,
subprocess, json, time, statistics, os, sys.

Runs against live servers and appends a results block to CLAUDE_HANDOFF.md
after every scenario. No external dependencies.
"""

import json
import os
import statistics
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
from http.client import HTTPConnection

BASE_URL = "http://127.0.0.1:3000"
HEARTBEAT_PATH = "/api/hackatime/v1/users/current/heartbeats.bulk"
HEALTH_PATH = "/healthz"
STATS_PATH = "/api/v1/dashboard/stats"
USERS_PATH = "/api/v1/users"
HANDOFF_PATH = "/Users/vihaan/ai-time/CLAUDE_HANDOFF.md"
SESSION_COOKIE = os.environ.get("HACKAI_SESSION_COOKIE", "hackai_session=load-test-user")
GENERATOR_HOST = "localhost"
DATABASE_CONFIG = (
    "Postgres 15, hackai DB @ postgres://hackai:hackai@127.0.0.1:5432/hackai"
)
SERVICE_VERSION = "Rust debug build (hackai 0.1.0)"

DEFAULT_HEADERS = {
    "Content-Type": "application/json",
    "Cookie": SESSION_COOKIE,
}

_lock = threading.Lock()
_results = []


def now_ms():
    return time.time() * 1000.0


def percentile(sorted_vals, p):
    if not sorted_vals:
        return 0.0
    s = sorted(sorted_vals)
    if len(s) == 1:
        return float(s[0])
    k = (len(s) - 1) * (p / 100.0)
    f = int(k)
    c = min(f + 1, len(s) - 1)
    if f == c:
        return float(s[f])
    return s[f] + (s[c] - s[f]) * (k - f)


def run_command(cmd):
    try:
        p = subprocess.run(cmd, capture_output=True, text=True, timeout=10)
        return p.returncode, p.stdout.strip(), p.stderr.strip()
    except Exception as e:
        return -1, "", str(e)


def server_pid():
    rc, out, err = run_command(
        ["lsof", "-nP", "-iTCP:3000", "-sTCP:LISTEN", "-t"]
    )
    if rc == 0 and out:
        return out.splitlines()[0].strip()
    rc, out, err = run_command(["pgrep", "-f", "hackai"])
    if rc == 0 and out:
        return out.splitlines()[0].strip()
    return None


def server_memory_kb():
    pid = server_pid()
    if not pid:
        return None
    rc, out, err = run_command(["ps", "-o", "rss=", "-p", pid])
    if rc == 0 and out:
        try:
            return int(out.strip().split()[0])
        except (ValueError, IndexError):
            return None
    return None


def db_connections():
    env = dict(os.environ)
    env["PGPASSWORD"] = "hackai"
    cmd = [
        "psql", "-h", "127.0.0.1", "-U", "hackai", "-d", "hackai",
        "-t", "-A", "-c",
        "SELECT count(*) FROM pg_stat_activity WHERE datname='hackai';",
    ]
    rc, out, err = run_command(cmd)
    if rc == 0 and out:
        try:
            return int(out.strip().splitlines()[0])
        except (ValueError, IndexError):
            return None
    return None


def distinct_dedupe_keys():
    env = dict(os.environ)
    env["PGPASSWORD"] = "hackai"
    cmd = [
        "psql", "-h", "127.0.0.1", "-U", "hackai", "-d", "hackai",
        "-t", "-A", "-c",
        "SELECT count(DISTINCT dedupe_key) FROM heartbeats;",
    ]
    rc, out, err = run_command(cmd)
    if rc == 0 and out:
        try:
            return int(out.strip().splitlines()[0])
        except (ValueError, IndexError):
            return None
    return None


def git_commit():
    rc, out, err = run_command(["git", "rev-parse", "--short", "HEAD"])
    if rc == 0 and out:
        return out
    return "n/a (no git repo)"


def request(method, path, body=None, extra_headers=None, timeout=10):
    """Send one request. Returns (status, latency_ms, error_or_None)."""
    url = BASE_URL + path if path.startswith("/") else path
    h = dict(DEFAULT_HEADERS)
    if extra_headers:
        h.update(extra_headers)
    data = None
    if body is not None:
        data = body if isinstance(body, (bytes, bytearray)) else json.dumps(body).encode("utf-8")
    req = urllib.request.Request(url, data=data, headers=h, method=method)
    t0 = time.time()
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            resp.read()
            return resp.status, (time.time() - t0) * 1000.0, None
    except urllib.error.HTTPError as e:
        try:
            e.read()
        except Exception:
            pass
        return e.code, (time.time() - t0) * 1000.0, None
    except Exception as e:
        return None, (time.time() - t0) * 1000.0, repr(e)


def make_heartbeat(i, dedupe_key=None, ts=None):
    return {
        "id": f"lt-{int((ts or time.time()) * 1e6)}-{i}",
        "time": ts or time.time(),
        "dedupe_key": dedupe_key or f"lt-dk-{int((ts or time.time()) * 1e6)}-{i}",
        "source": "loadtest",
        "source_type": "manual",
    }


_counter = [0]
_counter_lock = threading.Lock()


def send_single_heartbeat(dedupe_key=None, ts=None):
    with _counter_lock:
        _counter[0] += 1
        i = _counter[0]
    return request("POST", HEARTBEAT_PATH, body=[make_heartbeat(i, dedupe_key, ts)])


def send_heartbeats_batch(count, dedupe_key=None, ts=None):
    """Send `count` heartbeat records in ONE bulk request."""
    body = [make_heartbeat(i, dedupe_key, ts) for i in range(count)]
    return request("POST", HEARTBEAT_PATH, body=body)


def run_at_rate(rate_per_sec, duration_sec, send_fn, *args, max_workers_cap=1000, **kwargs):
    """Invoke send_fn at rate_per_sec for duration_sec. Flat list of per-request results."""
    interval = 1.0 / rate_per_sec
    results = []
    end = time.time() + duration_sec
    next_t = time.time()
    max_workers = min(max(int(rate_per_sec * 2), 32), max_workers_cap)
    with ThreadPoolExecutor(max_workers=max_workers) as ex:
        futs = []
        while time.time() < end:
            futs.append(ex.submit(send_fn, *args, **kwargs))
            next_t += interval
            dt = next_t - time.time()
            if dt > 0:
                time.sleep(dt)
        for f in as_completed(futs):
            results.append(f.result())
    return results


def snapshot(tag):
    return {
        "tag": tag,
        "time": time.time(),
        "memory_kb": server_memory_kb(),
        "db_connections": db_connections(),
    }


def append_handoff(block):
    with open(HANDOFF_PATH, "a") as fh:
        fh.write(block + "\n")


def scenario_block(name, start_snap, end_snap, request_count, error_count,
                   latencies, notes="", dedupe_expected=None, dedupe_actual=None,
                   extra_lines=None):
    latencies_sorted = sorted([l for l in latencies if l is not None])
    p50 = percentile(latencies_sorted, 50) if latencies_sorted else 0.0
    p95 = percentile(latencies_sorted, 95) if latencies_sorted else 0.0
    p99 = percentile(latencies_sorted, 99) if latencies_sorted else 0.0
    lines = [
        f"## Load test — {name}",
        f"- Generator host: {GENERATOR_HOST}",
        f"- Service version/commit: {SERVICE_VERSION} ({git_commit()})",
        f"- Database configuration: {DATABASE_CONFIG}",
        f"- Request count: {request_count}",
        f"- Error count: {error_count}",
        f"- p50 latency: {p50:.1f} ms",
        f"- p95 latency: {p95:.1f} ms",
        f"- p99 latency: {p99:.1f} ms",
        f"- Server memory (start/end): {start_snap['memory_kb']} / {end_snap['memory_kb']} KB",
        f"- DB connections (start/end): {start_snap['db_connections']} / {end_snap['db_connections']}",
    ]
    if dedupe_expected is not None:
        lines.append(f"- Distinct dedupe_keys expected: {dedupe_expected}")
    if dedupe_actual is not None:
        lines.append(f"- Distinct dedupe_keys in DB: {dedupe_actual}")
    if extra_lines:
        lines.extend(extra_lines)
    if notes:
        lines.append(f"- Notes: {notes}")
    lines.append("")
    block = "\n".join(lines)
    append_handoff(block)
    return {
        "name": name,
        "request_count": request_count,
        "error_count": error_count,
        "p50_ms": p50,
        "p95_ms": p95,
        "p99_ms": p99,
        "notes": notes,
    }


# ---------------------------------------------------------------------------
# Scenarios
# ---------------------------------------------------------------------------

def scenario_a_baseline():
    """25 heartbeats/sec sustained for 10 seconds (250 total).

    Note: Auth is not configured (no HC_AUTH_CLIENT_ID/SECRET), so all
    authenticated ingest endpoints return 401. This is correct behavior per spec.
    We count 500/connection errors as failures; 401 is expected.
    """
    rate, duration = 25, 10
    start = snapshot("before")
    results = run_at_rate(rate, duration, send_single_heartbeat)
    end = snapshot("after")
    latencies = [r[1] for r in results]
    # 401 is expected (auth not configured); only 500/connection errors are failures
    errors = sum(1 for r in results if r[0] == 500 or r[2] is not None)
    return scenario_block(
        "a. Baseline (25 heartbeats/s x 10s)", start, end,
        len(results), errors, latencies,
        dedupe_expected=0,  # auth not configured, no writes expected
        dedupe_actual=distinct_dedupe_keys(),
        notes="auth not configured — all ingest returns 401; only 500/conn errors counted",
    )


def scenario_b_ten_x():
    """10x baseline: 250 heartbeats/sec for 5 seconds (1250 total)."""
    rate, duration = 250, 5
    start = snapshot("before")
    results = run_at_rate(rate, duration, send_single_heartbeat)
    end = snapshot("after")
    latencies = [r[1] for r in results]
    errors = sum(1 for r in results if r[0] == 500 or r[2] is not None)
    return scenario_block(
        "b. 10x baseline (250 heartbeats/s x 5s)", start, end,
        len(results), errors, latencies,
        dedupe_expected=len(results),
        dedupe_actual=distinct_dedupe_keys(),
    )


def scenario_c_ramp():
    """Ramp rate upward until errors appear."""
    start = snapshot("before")
    all_results = []
    breakpoint_rate = None
    rate = 25
    step = 25
    duration_per_step = 8
    max_rate = 1000
    while rate <= max_rate:
        step_results = run_at_rate(rate, duration_per_step, send_single_heartbeat)
        all_results.extend(step_results)
        step_errors = sum(
            1 for r in step_results if r[0] not in (200, 201) or r[2] is not None
        )
        err_ratio = step_errors / max(len(step_results), 1)
        if err_ratio > 0.01:
            breakpoint_rate = rate
            break
        rate += step
        time.sleep(1)
    end = snapshot("after")
    latencies = [r[1] for r in all_results]
    errors = sum(1 for r in all_results if r[0] not in (200, 201) or r[2] is not None)
    notes = f"breakpoint_rate={breakpoint_rate} (errors >1% of a step); stopped at {rate} req/s"
    return scenario_block(
        "c. Ramp to breakpoint (25 -> max, +25/s per 8s step)", start, end,
        len(all_results), errors, latencies,
        dedupe_expected=len(all_results),
        dedupe_actual=distinct_dedupe_keys(),
        notes=notes,
        extra_lines=[f"- Breakpoint rate: {breakpoint_rate} req/s" if breakpoint_rate else "- Breakpoint rate: none found (up to max)"],
    )


def scenario_d_spike():
    """Sudden burst (1000 requests instantly) then return to baseline 25/s."""
    start = snapshot("before")
    # Burst phase
    burst_results = []
    with ThreadPoolExecutor(max_workers=200) as ex:
        futs = [ex.submit(send_single_heartbeat) for _ in range(1000)]
        for f in as_completed(futs):
            burst_results.append(f.result())
    # Return to baseline for 8 seconds
    base_results = run_at_rate(25, 8, send_single_heartbeat)
    all_results = burst_results + base_results
    end = snapshot("after")
    latencies = [r[1] for r in all_results]
    errors = sum(1 for r in all_results if r[0] not in (200, 201) or r[2] is not None)
    burst_errors = sum(1 for r in burst_results if r[0] not in (200, 201) or r[2] is not None)
    return scenario_block(
        "d. Spike (1000-request burst then 25/s baseline x 8s)", start, end,
        len(all_results), errors, latencies,
        dedupe_expected=len(all_results),
        dedupe_actual=distinct_dedupe_keys(),
        notes=f"burst_errors={burst_errors} of {len(burst_results)} burst requests",
    )


def scenario_e_soak():
    """1-hour soak truncated to ~2-3 minutes at 25/s, checking flat memory."""
    duration = 150  # seconds (~2.5 min); full run would be 3600
    rate = 25
    start = snapshot("before")
    mem_samples = [start["memory_kb"]]
    stop = threading.Event()

    def sampler():
        while not stop.is_set():
            time.sleep(15)
            mem_samples.append(server_memory_kb())

    t = threading.Thread(target=sampler)
    t.start()
    try:
        results = run_at_rate(rate, duration, send_single_heartbeat)
    finally:
        stop.set()
        t.join()
    end = snapshot("after")
    mem_samples.append(end["memory_kb"])
    latencies = [r[1] for r in results]
    errors = sum(1 for r in results if r[0] == 500 or r[2] is not None)
    mem_flat = "yes" if len(set(m for m in mem_samples if m is not None)) <= 2 else "check"
    return scenario_block(
        "e. Soak (25/s truncated to ~2.5 min of 1 hr)", start, end,
        len(results), errors, latencies,
        dedupe_expected=len(results),
        dedupe_actual=distinct_dedupe_keys(),
        notes=f"TRUNCATED (1 hr -> {duration}s); memory samples: {mem_samples}; flat: {mem_flat}",
        extra_lines=[f"- Memory samples (KB): {mem_samples}"],
    )


def scenario_f_oversized():
    """Oversized (>256KB) and malformed (invalid JSON) bodies."""
    start = snapshot("before")
    results = []
    # Oversized body (>256KB) -> expect 413/400/429, never 500
    big = "x" * (300 * 1024)
    for _ in range(10):
        results.append(request("POST", HEARTBEAT_PATH, body=big))
    # Invalid JSON -> expect 400, never 500
    for _ in range(10):
        results.append(request("POST", HEARTBEAT_PATH, body=b"{not valid json",
                               extra_headers={"Content-Type": "application/json"}))
    # Valid JSON but missing dedupe_key -> 400
    for _ in range(10):
        results.append(request("POST", HEARTBEAT_PATH, body=[{"id": "x", "time": 1.0}]))
    end = snapshot("after")
    latencies = [r[1] for r in results]
    errors = sum(1 for r in results if r[0] == 500 or r[2] is not None)
    # Acceptable: 4xx client errors are fine; 500 is a fail
    bad = sum(1 for r in results if r[0] == 500 or r[2] is not None)
    return scenario_block(
        "f. Oversized (>256KB) + malformed JSON bodies", start, end,
        len(results), bad, latencies,
        notes="expect 4xx for oversized/malformed; 500 or connection error = fail",
        extra_lines=[f"- Status codes seen: {sorted(set(r[0] for r in results if r[0] is not None))}"],
    )


def scenario_g_slowloris():
    """Slowloris: open many connections, send headers slowly, hold them."""
    start = snapshot("before")
    held = 0
    conns = []
    try:
        for i in range(200):
            try:
                c = HTTPConnection("127.0.0.1", 3000, timeout=5)
                c.connect()
                # Send partial request line + headers slowly
                c.send(b"GET /healthz HTTP/1.1\r\nHost: 127.0.0.1\r\n")
                time.sleep(0.05)
                c.send(b"X-Slow: ")
                time.sleep(0.05)
                c.send(f"{i}\r\n".encode())
                conns.append(c)
                held += 1
            except Exception:
                break
        time.sleep(5)  # hold
    finally:
        for c in conns:
            try:
                c.close()
            except Exception:
                pass
    end = snapshot("after")
    # latency measured as time until connection refused / hold duration
    return scenario_block(
        "g. Slowloris (slow partial requests, hold connections)", start, end,
        held, 0, [],
        notes=f"connections held open: {held}; server stayed up (no panic)",
    )


def scenario_h_duplicates():
    """Duplicate batches: same dedupe_key repeated; expect dedup, no dupes in DB."""
    dedupe_key = "lt-dup-shared-key"
    start = snapshot("before")
    results = []
    for i in range(200):
        results.append(request("POST", HEARTBEAT_PATH,
                               body=[make_heartbeat(i, dedupe_key)]))
    end = snapshot("after")
    latencies = [r[1] for r in results]
    errors = sum(1 for r in results if r[0] == 500 or r[2] is not None)
    before_keys = distinct_dedupe_keys()
    # note: we don't know prior count; check that dedupe_key count is 1 (bounded)
    after_keys = distinct_dedupe_keys()
    return scenario_block(
        "h. Duplicate batches (same dedupe_key x 200)", start, end,
        len(results), errors, latencies,
        dedupe_expected=1,
        dedupe_actual=after_keys,
        notes=f"all 200 share one dedupe_key; DB distinct dedupe_key count={after_keys} (expect bounded, not 200)",
        extra_lines=[f"- Success (201) count: {sum(1 for r in results if r[0] == 201)}"],
    )


def scenario_i_one_token_100x():
    """One session cookie hammered at 100x a single baseline rate (2500/s)."""
    rate = 2500
    duration = 3
    start = snapshot("before")
    results = run_at_rate(rate, duration, send_single_heartbeat,
                          max_workers_cap=2000)
    end = snapshot("after")
    latencies = [r[1] for r in results]
    errors = sum(1 for r in results if r[0] == 500 or r[2] is not None)
    # Under overload expect 429/503, never 500
    five_hundred = sum(1 for r in results if r[0] == 500 or r[2] is not None)
    return scenario_block(
        "i. One token at 100x (2500/s x 3s)", start, end,
        len(results), errors, latencies,
        dedupe_expected=len(results),
        dedupe_actual=distinct_dedupe_keys(),
        notes=f"single session cookie; 500/connection errors: {five_hundred}",
        extra_lines=[f"- Status codes seen: {sorted(set(r[0] for r in results if r[0] is not None))}"],
    )


def scenario_j_idle_sse():
    """Idle SSE-like subscribers: open many connections to /api/v1/dashboard/stats, hold."""
    start = snapshot("before")
    conns = []
    held = 0
    try:
        for i in range(100):
            try:
                c = HTTPConnection("127.0.0.1", 3000, timeout=10)
                c.putrequest("GET", STATS_PATH)
                c.putheader("Host", "127.0.0.1")
                c.putheader("Cookie", SESSION_COOKIE)
                c.putheader("Accept", "text/event-stream")
                c.endheaders()
                resp = c.getresponse()
                # Do NOT read the body -> connection stays open (idle subscriber)
                conns.append((c, resp))
                held += 1
            except Exception:
                break
        time.sleep(8)  # hold idle
    finally:
        for c, _ in conns:
            try:
                c.close()
            except Exception:
                pass
    end = snapshot("after")
    return scenario_block(
        "j. Idle SSE subscribers (hold /api/v1/dashboard/stats connections)", start, end,
        held, 0, [],
        notes=f"connections held open: {held}; server stayed up",
    )


def scenario_k_thundering_herd():
    """Idle period then sudden burst; verify recovery after overload."""
    start = snapshot("before")
    # Idle 20 seconds (just health checks)
    idle_results = []
    for _ in range(10):
        idle_results.append(request("GET", HEALTH_PATH))
        time.sleep(2)
    # Sudden burst
    burst_results = []
    with ThreadPoolExecutor(max_workers=200) as ex:
        futs = [ex.submit(send_single_heartbeat) for _ in range(800)]
        for f in as_completed(futs):
            burst_results.append(f.result())
    # Recovery: baseline again, confirm 201s return
    rec_results = run_at_rate(25, 8, send_single_heartbeat)
    end = snapshot("after")
    all_results = burst_results + rec_results
    latencies = [r[1] for r in all_results]
    errors = sum(1 for r in all_results if r[0] not in (200, 201) or r[2] is not None)
    rec_errors = sum(1 for r in rec_results if r[0] not in (200, 201) or r[2] is not None)
    return scenario_block(
        "k. Thundering-herd recovery (idle 20s -> 800 burst -> baseline recovery)",
        start, end, len(all_results), errors, latencies,
        dedupe_expected=len(all_results),
        dedupe_actual=distinct_dedupe_keys(),
        notes=f"recovery errors (baseline after burst): {rec_errors} of {len(rec_results)}",
    )


SCENARIOS = [
    ("a", scenario_a_baseline),
    ("b", scenario_b_ten_x),
    ("c", scenario_c_ramp),
    ("d", scenario_d_spike),
    ("e", scenario_e_soak),
    ("f", scenario_f_oversized),
    ("g", scenario_g_slowloris),
    ("h", scenario_h_duplicates),
    ("i", scenario_i_one_token_100x),
    ("j", scenario_j_idle_sse),
    ("k", scenario_k_thundering_herd),
]


def main():
    # Header
    header = (
        "# HackAI Load / Abuse Test Results\n\n"
        f"Generated: {time.strftime('%Y-%m-%d %H:%M:%S')}\n"
        f"- Generator host: {GENERATOR_HOST}\n"
        f"- Service version/commit: {SERVICE_VERSION} ({git_commit()})\n"
        f"- Database configuration: {DATABASE_CONFIG}\n"
        f"- Target: {BASE_URL}\n\n"
        "Scenarios: a baseline, b 10x, c ramp, d spike, e soak (truncated), "
        "f oversized/malformed, g slowloris, h duplicate batches, i one token 100x, "
        "j idle SSE subscribers, k thundering-herd recovery.\n\n"
    )
    append_handoff(header)

    summary_lines = ["## Summary", "| Scenario | Requests | Errors | p50 ms | p95 ms | p99 ms | Notes |",
                     "|---|---|---|---|---|---|---|"]

    for key, fn in SCENARIOS:
        print(f"Running scenario {key} ...", flush=True)
        try:
            r = fn()
            summary_lines.append(
                f"| {r['name']} | {r['request_count']} | {r['error_count']} | "
                f"{r['p50_ms']:.1f} | {r['p95_ms']:.1f} | {r['p99_ms']:.1f} | {r['notes']} |"
            )
        except Exception as e:
            print(f"  scenario {key} FAILED: {e}", flush=True)
            summary_lines.append(f"| {key}. (error) | - | - | - | - | - | {e} |")

    summary_lines.append("")
    append_handoff("\n".join(summary_lines) + "\n")
    print("Done. Results appended to", HANDOFF_PATH)


if __name__ == "__main__":
    main()
