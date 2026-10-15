import secrets
import sys

from common import alter_role_verifier, load_secret, password_of, tcp_login_ok, with_password, write_secret
from scram import scram_sha256_verifier

name, role = sys.argv[1], sys.argv[2]
old = load_secret(name)
if load_secret(name + "_PREV") != old:
    raise SystemExit("PREV copy missing or differs, abort")
new_password = secrets.token_urlsafe(48)
new = with_password(old, new_password)
if password_of(new) != new_password or with_password(new, password_of(old)) != old:
    raise SystemExit("DSN rebuild is not lossless, abort")
write_secret(name, new)
print("infisical updated and read back equal")
alter_role_verifier(role, scram_sha256_verifier(new_password))
print("role password altered")
print("new_login_ok", tcp_login_ok(new))
print("old_login_rejected", not tcp_login_ok(old))
