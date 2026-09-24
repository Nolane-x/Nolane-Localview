#!/usr/bin/env python3
import argparse
import json
import sys
import time
import urllib.error
import urllib.request


def read_health():
    request = urllib.request.Request(
        "http://127.0.0.1:45454/health",
        headers={"User-Agent": "localview-release-smoke/1"},
        method="GET",
    )
    with urllib.request.urlopen(request, timeout=1.0) as response:
        if response.status != 200:
            raise RuntimeError(f"unexpected health status {response.status}")
        if response.geturl() != "http://127.0.0.1:45454/health":
            raise RuntimeError("health endpoint redirected")
        body = response.read(64 * 1024 + 1)
        if len(body) > 64 * 1024:
            raise RuntimeError("health response exceeds bound")
        return json.loads(body)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--timeout-seconds", type=float, default=30.0)
    args = parser.parse_args()
    if args.timeout_seconds <= 0 or args.timeout_seconds > 120:
        raise SystemExit("timeout must be within (0, 120] seconds")

    deadline = time.monotonic() + args.timeout_seconds
    last_error = "no attempt"
    while time.monotonic() < deadline:
        try:
            health = read_health()
            if health.get("status") != "ready":
                last_error = f"daemon status is {health.get('status')!r}, expected 'ready'"
            elif health.get("version") != args.version:
                last_error = (
                    f"daemon version is {health.get('version')!r}, "
                    f"expected {args.version!r}"
                )
            else:
                print(json.dumps(health, sort_keys=True))
                return 0
        except (OSError, ValueError, RuntimeError, urllib.error.URLError) as error:
            last_error = str(error)
        time.sleep(0.25)

    print(f"LocalView first-launch health smoke failed: {last_error}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
