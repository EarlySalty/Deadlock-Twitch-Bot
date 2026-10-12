import datetime
import json
import pathlib
import subprocess
import sys
import time

name, cwd, *command = sys.argv[1:]
root = pathlib.Path(__file__).parent
start = datetime.datetime.now(datetime.timezone.utc).isoformat()
clock = time.monotonic()
with (root / f"{name}.log").open("w") as output:
    result = subprocess.run(command, cwd=cwd, stdout=output, stderr=subprocess.STDOUT)
evidence = {"command": command, "cwd": cwd, "started_at": start, "ended_at": datetime.datetime.now(datetime.timezone.utc).isoformat(), "duration_seconds": round(time.monotonic() - clock, 3), "exit": result.returncode}
(root / f"{name}.json").write_text(json.dumps(evidence, indent=2) + "\n")
print(json.dumps(evidence))
sys.exit(result.returncode)
