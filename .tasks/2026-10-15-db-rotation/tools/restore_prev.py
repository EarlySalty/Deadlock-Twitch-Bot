import sys

from common import load_secret, tcp_login_ok, write_secret

name = sys.argv[1]
previous = load_secret(name + "_PREV")
write_secret(name, previous)
print(f"{name} restored from {name}_PREV")
print("prev_login_ok", tcp_login_ok(previous))
