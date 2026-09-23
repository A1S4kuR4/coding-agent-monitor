"""T07 Windows-only isolated synthetic worker benchmark. Never starts the GUI.
Run: python -X utf8 scripts/history-benchmark.py --runs 30
The long-log Claude source has an explicit temporary root; stdout contains metrics only.
"""
import argparse
import ctypes as c
from ctypes import wintypes as w
import datetime as dt
import json
import os
from pathlib import Path
import statistics
import subprocess
import tempfile
import threading
import time

class Memory(c.Structure):
    _fields_ = [("cb", w.DWORD), ("faults", w.DWORD)] + [(name, c.c_size_t) for name in
        ("peak", "working", "paged_peak", "paged", "nonpaged_peak", "nonpaged", "pagefile", "peak_pagefile")]
psapi = c.WinDLL("psapi", use_last_error=True)
psapi.GetProcessMemoryInfo.argtypes = [w.HANDLE, c.POINTER(Memory), w.DWORD]
kernel = c.WinDLL("kernel32", use_last_error=True)
kernel.GetProcessTimes.argtypes = [w.HANDLE] + [c.POINTER(w.FILETIME)] * 4

def measure(exe, req, env):
    start = time.perf_counter()
    p = subprocess.Popen([str(exe), "--cam-internal-collector-worker-v1"], stdin=subprocess.PIPE,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, creationflags=subprocess.CREATE_NO_WINDOW)
    output = []
    def collect():
        try:
            output.append(p.communicate(json.dumps(req).encode(), timeout=120))
        except subprocess.TimeoutExpired:
            p.kill(); p.communicate(); output.append(None)
    thread = threading.Thread(target=collect); thread.start()
    peak = 0
    while thread.is_alive():
        mem = Memory(); mem.cb = c.sizeof(mem)
        if psapi.GetProcessMemoryInfo(int(p._handle), c.byref(mem), mem.cb): peak = max(peak, mem.peak)
        thread.join(0.005)
    elapsed = (time.perf_counter() - start) * 1000
    times = [w.FILETIME() for _ in range(4)]
    assert kernel.GetProcessTimes(int(p._handle), *(c.byref(t) for t in times))
    cpu = sum((t.dwHighDateTime << 32) + t.dwLowDateTime for t in times[2:]) / 10000
    assert output[0] is not None and p.returncode == 0, "worker failure (raw output suppressed)"
    response = json.loads(output[0][0])
    assert not response.get("fatal_error") and len(response["agents"]) == len(req["agents"])
    assert all(a["status"] == "ok" for a in response["agents"]), "source error (suppressed)"
    rows = sum(len(a["report"]["records"]) for a in response["agents"])
    expected_rows = (dt.date.fromisoformat(req["window"]["end_inclusive"]) - dt.date.fromisoformat(req["window"]["start_inclusive"])).days + 1
    assert rows == expected_rows, "synthetic records were not collected; benchmark invalid"
    assert all(not a["report"].get("diagnostics") for a in response["agents"]), str([(a["agent"], [d["kind"] for d in a["report"].get("diagnostics", [])]) for a in response["agents"] if a["report"].get("diagnostics")])
    return elapsed, cpu, peak / 1048576, rows

def main():
    parser = argparse.ArgumentParser(); parser.add_argument("--runs", type=int, default=30)
    args = parser.parse_args()
    exe = Path(__file__).resolve().parents[1] / "src-tauri/target/release/coding-agent-monitor.exe"
    agents = ["claude"]
    with tempfile.TemporaryDirectory(prefix="cam-t07-benchmark-") as temp:
        root = Path(temp); roots = {agent: root / agent for agent in agents}
        roots["claude"].mkdir()
        data = roots["claude"] / "projects" / "synthetic"; data.mkdir(parents=True)
        log = data / "synthetic.jsonl"
        end = dt.date(2026, 1, 3)
        with log.open("w", encoding="utf-8") as f:
            for i in range(100000):
                date = end - dt.timedelta(days=i % 60)
                f.write(json.dumps({"timestamp": f"{date}T12:00:00Z", "sessionId": "synthetic", "requestId": f"test-{i}",
                    "message": {"id": f"test-{i}", "model": "claude-sonnet-4-20250514", "usage": {
                        "input_tokens": 100, "output_tokens": 10, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}}}, separators=(",", ":")) + "\n")
        env = dict(os.environ)
        # Explicit roots are authoritative; also scrub test hooks inherited by a development shell.
        for key in list(env):
            if key.startswith("CAM_TEST_"): del env[key]
        results = {7: [], 30: []}
        for run in range(args.runs):
            for days in ([7,30] if run % 2 == 0 else [30,7]):
                req = {"version": 1, "request_id": f"bench-{run}-{days}", "agents": [
                    {"agent": a, "source": {"kind": "paths", "roots": [str(roots[a])]}} for a in agents],
                    "window": {"start_inclusive": str(end-dt.timedelta(days=days-1)), "end_inclusive": str(end)}, "timezone": "UTC"}
                results[days].append(measure(exe, req, env))
            print(f"completed paired run {run+1}/{args.runs}", flush=True)
        print(json.dumps({"synthetic_records": 100000, "bytes": log.stat().st_size, "runs_per_range": args.runs,
            "metrics": {str(days): {"p50_ms": statistics.median(r[0] for r in rows),
              "p95_ms": sorted(r[0] for r in rows)[__import__("math").ceil(len(rows)*.95)-1],
              "cpu_p50_ms": statistics.median(r[1] for r in rows), "peak_working_set_mib": max(r[2] for r in rows),
              "daily_rows": sorted(set(r[3] for r in rows))} for days, rows in results.items()}}, indent=2))
if __name__ == "__main__": main()
