import sys

from common import load_secret, tcp_login_ok

name = sys.argv[1]
current = load_secret(name)
previous = load_secret(name + "_PREV")
print("same_value", current == previous)
print("current_login_ok", tcp_login_ok(current))
print("prev_login_ok", tcp_login_ok(previous))
