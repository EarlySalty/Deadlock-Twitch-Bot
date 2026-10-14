import argparse
import json
import subprocess
import sys
import tomllib
from pathlib import Path


def inspect(binary, path):
    result = subprocess.run(
        ["sudo", "-n", str(binary), "--config", str(path), "--social-ai-inspect"],
        capture_output=True,
        check=False,
    )
    if result.returncode:
        raise RuntimeError("Configuration inspection failed; no file contents emitted")
    return json.loads(result.stdout)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--previous-release", type=Path, required=True)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    old = args.previous_release / "ops" / "social-ai"
    editor_text = (old / "editor.toml").read_text()
    pricing_text = (old / "pricing-v1.toml").read_text()
    text = editor_text + "\n[pricing]\n" + pricing_text.replace("[pricing]", "[pricing.rates]")
    section = tomllib.loads(text)
    editor = tomllib.loads(editor_text)
    pricing = tomllib.loads(pricing_text)
    expected = dict(editor, pricing={"version": pricing["version"], "model": pricing["model"], "rates": pricing["pricing"]})
    if section != expected or "brain" in section["runtime"]:
        raise RuntimeError("Social AI migration must preserve the existing settings without Brain")
    path = Path("/var/lib/deadlock-twitch/config/bot.toml")
    before = inspect(args.binary, path)
    if before["social_ai"] == section:
        print("SOCIAL_AI_ALREADY_PRESENT exact_previous_settings=true brain_absent=true")
        return
    if before["social_ai"] is not None:
        raise RuntimeError("Existing Social AI section differs; migration stopped without overwriting")
    if not args.apply:
        print("SOCIAL_AI_MIGRATION_READY previous_values_preserved=true brain_absent=true")
        return
    applied = subprocess.run(
        ["sudo", "-n", str(args.binary), "--config", str(path), "--social-ai-apply", "--expected-revision", before["revision"]],
        input=text.encode(),
        capture_output=True,
        check=False,
    )
    if applied.returncode:
        raise RuntimeError("Hash-checked Social AI update failed; no file contents emitted")
    after = inspect(args.binary, path)
    backup = inspect(args.binary, path.parent / ".bot.toml.previous")
    if after["social_ai"] != section or backup["revision"] != before["revision"]:
        raise RuntimeError("Social AI update or backup verification failed")
    print("SOCIAL_AI_MIGRATED exact_previous_settings=true brain_absent=true backup_verified=true revision=" + after["revision"])


try:
    main()
except (OSError, RuntimeError, ValueError, KeyError):
    print("Social AI migration could not be confirmed. No configuration contents emitted.", file=sys.stderr)
    sys.exit(2)
