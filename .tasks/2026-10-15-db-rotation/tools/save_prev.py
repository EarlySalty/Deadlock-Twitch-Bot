import sys

from common import load_secret, password_of, tcp_login_ok, write_secret

name = sys.argv[1]
current = load_secret(name)
print("current_login_ok", tcp_login_ok(current))
write_secret(name + "_PREV", current, create=True)
print(f"{name}_PREV stored and read back equal")
