#!/usr/bin/env python3
"""Run the original yydbmanager demo against the terminal database worker."""
import argparse
import datetime
import json
import os
from pathlib import Path
import subprocess
import tomllib

REPO = Path(__file__).resolve().parents[1]
SERVICES = ("postgres", "mysql", "sqlserver", "oracle", "db2")

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--yyeditor", type=Path, required=True, help="Checkout of the pinned yyeditor revision")
    parser.add_argument("--up", action="store_true", help="Start/wait for the ORIGINAL Compose; never creates a copy")
    parser.add_argument("--host", default="127.0.0.1", help="Published-port host (host.docker.internal inside Docker Desktop)")
    args = parser.parse_args()
    source = args.yyeditor.resolve()
    demo = source / "demo" / "yydbmanager"
    expected = tomllib.loads((REPO / "Cargo.toml").read_text(encoding="utf-8"))["dependencies"]["yy-db-core"]["rev"]
    actual = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
    if actual != expected:
        parser.error(f"yyeditor revision differs from yy-db-core: {actual} != {expected}")
    if not (demo / "compose.yaml").is_file():
        parser.error("Original demo/yydbmanager/compose.yaml is missing")
    if subprocess.run(["git", "-C", str(source), "diff", "--quiet", "HEAD", "--", "demo/yydbmanager"]).returncode:
        parser.error("yyeditor demo files differ from the pinned revision; use the unchanged original assets")
    stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
    report_dir = REPO / "target" / "database-live" / stamp
    report_dir.mkdir(parents=True)
    compose = ["docker", "compose", "-p", "yydbmanager-demo", "-f", str(demo / "compose.yaml"), "--profile", "*"]
    report = {"yyeditor_revision": actual, "compose": str(demo / "compose.yaml"), "status": "failed", "services": [], "steps": []}
    def run(label, command, env=None):
        print(f"\n{label}", flush=True)
        result = subprocess.run(command, cwd=REPO, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace")
        print(result.stdout, end="", flush=True)
        (report_dir / f"{label}.log").write_text(result.stdout, encoding="utf-8")
        report["steps"].append({"name": label, "exit_code": result.returncode})
        if result.returncode:
            raise RuntimeError(f"{label} failed ({result.returncode})")
    try:
        if args.up:
            run("compose-up", compose + ["up", "-d", "--wait", "--wait-timeout", "1200"])
        for service in SERVICES:
            ident = subprocess.check_output(compose + ["ps", "-q", service], text=True).strip()
            if not ident:
                raise RuntimeError(f"{service}: start the original Compose with --up")
            info = json.loads(subprocess.check_output(["docker", "inspect", ident], text=True))[0]
            health = info["State"].get("Health", {}).get("Status")
            report["services"].append({"service": service, "container": ident, "image": info["Image"], "health": health})
            if health != "healthy":
                raise RuntimeError(f"{service}: not ready ({health}); inspect original Compose logs")
        env = os.environ.copy()
        env.update(YYDBMANAGER_DEMO=str(demo), YYDBMANAGER_HOST=args.host, YYDBMANAGER_REPORTDIR=str(report_dir), PYTHONUTF8="1")
        # Original vendor-client verification is run without modifications on Windows.
        if os.name == "nt":
            script = str(demo / "verify.ps1").replace("'", "''")
            run("original-demo-verification", ["pwsh", "-NoProfile", "-Command", f"& '{script}' -Services postgres,mysql,sqlserver,oracle,db2"], env)
        run("terminal-worker", ["cargo", "test", "--locked", "--test", "database_live", "--", "--ignored", "--nocapture", "--test-threads=1"], env)
        report["status"] = "passed"
        return 0
    except (OSError, subprocess.SubprocessError, RuntimeError) as error:
        report["error"] = str(error)
        print(error, flush=True)
        return 1
    finally:
        (report_dir / "verification.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(f"Report: {report_dir}", flush=True)

if __name__ == "__main__":
    raise SystemExit(main())
