import argparse
import json
import subprocess
import sys

parser = argparse.ArgumentParser()
parser.add_argument('--since', required=True)
args = parser.parse_args()
units = ['deadlock-twitch-bot-rust', 'deadlock-twitch-dashboard-rust', 'deadlock-twitch-stream-coaching-watch', 'deadlock-twitch-migrate']
control = subprocess.run(['journalctl', '--no-pager', '-u', units[0], '--since', args.since, '--output=json', '--output-fields=__REALTIME_TIMESTAMP,PRIORITY,_SYSTEMD_UNIT'], capture_output=True, text=True)
control_records = [json.loads(line) for line in control.stdout.splitlines() if line.strip()]
permissions_hint = 'not seeing messages' in control.stderr.lower() or 'permission' in control.stderr.lower()
if control.returncode or not control_records or permissions_hint:
    print(json.dumps({'proof': 'journal_errors', 'since': args.since, 'access_verified': False, 'positive_control_records': len(control_records), 'permissions_hint': permissions_hint, 'read_exit': control.returncode, 'all_empty': None}))
    sys.exit(3)
proof = []
for unit in units:
    result = subprocess.run(['journalctl', '--no-pager', '--quiet', '-u', unit, '-p', 'err', '--since', args.since, '--output=json', '--output-fields=__REALTIME_TIMESTAMP,PRIORITY,_SYSTEMD_UNIT'], capture_output=True, text=True)
    if result.returncode:
        print(json.dumps({'proof': 'journal_errors', 'unit': unit, 'read_exit': result.returncode}), file=sys.stderr)
        sys.exit(result.returncode)
    records = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
    proof.append({'unit': unit, 'error_records': len(records), 'timestamps': [record.get('__REALTIME_TIMESTAMP') for record in records]})
print(json.dumps({'proof': 'journal_errors', 'since': args.since, 'units': proof, 'all_empty': all(not row['error_records'] for row in proof)}))
sys.exit(0 if all(not row['error_records'] for row in proof) else 1)
