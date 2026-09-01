#!/usr/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALLER="$SCRIPT_DIR/install-twitch-release.sh"
BOT_UNIT="$SCRIPT_DIR/deadlock-twitch-bot-rust.service"
DASHBOARD_UNIT="$SCRIPT_DIR/deadlock-twitch-dashboard-rust.service"
DOWNLOADER_SOCKET="$SCRIPT_DIR/deadlock-twitch-media-downloader.socket"
DOWNLOADER_SERVICE="$SCRIPT_DIR/deadlock-twitch-media-downloader@.service"
DOWNLOADER_FIREWALL="$SCRIPT_DIR/deadlock-twitch-media-downloader-firewall.service"
DOWNLOADER_FIREWALL_REFRESH="$SCRIPT_DIR/deadlock-twitch-media-downloader-firewall-refresh@.service"
DOWNLOADER_FIREWALL_VALIDATOR="$SCRIPT_DIR/validate-tb-media-downloader-firewall.sh"
DOWNLOADER_RESOLVER="$SCRIPT_DIR/tb-media-downloader-resolv.conf"
DOWNLOADER_NFT="$SCRIPT_DIR/tb-media-downloader-egress.nft"
EXPECTED_INSTALLER_SHEBANG='#!/usr/bin/env -S -i PATH=/usr/sbin:/usr/bin:/sbin:/bin LC_ALL=C /usr/bin/bash --noprofile --norc'
if [[ "$(head -n 1 "$INSTALLER")" != "$EXPECTED_INSTALLER_SHEBANG" ]]; then
  echo "Root-Installer erzwingt vor Bash keine saubere Umgebung." >&2
  exit 1
fi
for nft_package_gate in \
  "NFT_PACKAGE_VERSION='1.0.9-1ubuntu0.1'" \
  '/usr/bin/dpkg --verify' \
  '/usr/bin/systemctl is-enabled nftables.service' \
  '/usr/bin/systemctl is-active nftables.service' \
  'generic_enabled" != disabled' \
  'generic_active" != inactive'; do
  if ! grep -Fq "$nft_package_gate" "$INSTALLER"; then
    echo "Installer prüft den gepinnten nftables-Paket-/Managerzustand nicht: $nft_package_gate" >&2
    exit 1
  fi
done
if grep -Eq 'systemctl[[:space:]]+(stop|disable)[[:space:]]+nftables' "$INSTALLER"; then
  echo "Installer darf den global flushenden nftables.service nie automatisch stoppen." >&2
  exit 1
fi
# Die statischen Muster müssen `$unit` bzw. `$current_tmp` wörtlich im
# Installer finden, nicht Variablen dieses Testprozesses expandieren.
# shellcheck disable=SC2016
for offline_gate in \
  'validate_offline_unit_gates' \
  'deadlock-twitch-bot-rust.service' \
  'deadlock-twitch-dashboard-rust.service' \
  'deadlock-twitch-media-downloader.socket' \
  'state" != masked-runtime' \
  '/run/systemd/system/$unit' \
  'Offline-Cutover erfordert eine persistent deaktivierte Unit'; do
  if ! grep -Fq "$offline_gate" "$INSTALLER"; then
    echo "Installer prüft das reboot-feste Offline-Gate nicht: $offline_gate" >&2
    exit 1
  fi
done
if grep -Eq '/usr/bin/systemctl[[:space:]]+(mask|unmask|enable|disable|start|stop|restart)' \
    "$INSTALLER"; then
  echo "Installer darf den Offline-Masken-/Lifecycle-Zustand nur prüfen." >&2
  exit 1
fi
for unit in "$BOT_UNIT" "$DASHBOARD_UNIT"; do
  if [[ "$(grep -c '^UMask=0027$' "$unit")" -ne 1 ]] || grep -q '^UMask=0007$' "$unit"; then
    echo "Systemd-Unit erzwingt nicht die getrennte Medien-UMask: $unit" >&2
    exit 1
  fi
done
for bot_limit in \
  'MemoryAccounting=yes' 'MemoryHigh=3G' 'MemoryMax=4G' \
  'CPUAccounting=yes' 'CPUQuota=300%'; do
  if [[ "$(grep -Fxc "$bot_limit" "$BOT_UNIT")" -ne 1 ]]; then
    echo "Bot-Unit fehlt das aggregierte Ressourcenlimit: $bot_limit" >&2
    exit 1
  fi
done
for dashboard_limit in \
  'MemoryAccounting=yes' 'MemoryHigh=2G' 'MemoryMax=3G' \
  'CPUAccounting=yes' 'CPUQuota=200%'; do
  if [[ "$(grep -Fxc "$dashboard_limit" "$DASHBOARD_UNIT")" -ne 1 ]]; then
    echo "Dashboard-Unit fehlt das aggregierte Ressourcenlimit: $dashboard_limit" >&2
    exit 1
  fi
done
for required_file in \
  "$DOWNLOADER_SOCKET" \
  "$DOWNLOADER_SERVICE" \
  "$DOWNLOADER_FIREWALL" \
  "$DOWNLOADER_FIREWALL_REFRESH" \
  "$DOWNLOADER_FIREWALL_VALIDATOR" \
  "$DOWNLOADER_RESOLVER" \
  "$DOWNLOADER_NFT"; do
  if [[ ! -f "$required_file" || -L "$required_file" ]]; then
    echo "Downloader-Infrastruktur fehlt oder ist ein Symlink: $required_file" >&2
    exit 1
  fi
done
for required_refresh_dependency in \
  'Requires=deadlock-twitch-media-downloader-firewall-refresh@%i.service' \
  'After=deadlock-twitch-media-downloader-firewall-refresh@%i.service'; do
  if [[ "$(grep -Fxc "$required_refresh_dependency" "$DOWNLOADER_SERVICE")" -ne 1 ]]; then
    echo "Downloader-Instanz startet ohne frischen nft-Gate-Reload: $required_refresh_dependency" >&2
    exit 1
  fi
done
for required_socket_dependency in \
  'Requires=deadlock-twitch-media-downloader-firewall.service' \
  'After=deadlock-twitch-media-downloader-firewall.service'; do
  if [[ "$(grep -Fxc "$required_socket_dependency" "$DOWNLOADER_SOCKET")" -ne 1 ]]; then
    echo "Downloader-Socket startet ohne Firewall-Gate: $required_socket_dependency" >&2
    exit 1
  fi
done
for required_socket_setting in \
  'ListenSequentialPacket=/run/deadlock-twitch-media-downloader/control.sock' \
  'Accept=yes' \
  'PassCredentials=yes' \
  'SocketUser=root' \
  'SocketGroup=twitchbot' \
  'SocketMode=0660' \
  'DirectoryMode=0755' \
  'RemoveOnStop=yes'; do
  if [[ "$(grep -Fxc "$required_socket_setting" "$DOWNLOADER_SOCKET")" -ne 1 ]]; then
    echo "Downloader-Socket fehlt die feste Grenze: $required_socket_setting" >&2
    exit 1
  fi
done
for required_service_setting in \
  'User=twitchdownload' \
  'Group=twitchdownload' \
  'ExecStart=/opt/deadlock/twitch/current/rust/target/release/tb-media-downloader --stdio' \
  'StandardInput=socket' \
  'RestrictNamespaces=user mnt pid' \
  'RestrictAddressFamilies=AF_INET AF_INET6' \
  'TasksMax=32' \
  'MemoryMax=2304M' \
  'MemorySwapMax=0' \
  'CPUQuota=200%' \
  'LimitNOFILE=128' \
  'LimitFSIZE=536870912' \
  'BindReadOnlyPaths=/opt/deadlock/twitch/current/ops/systemd/tb-media-downloader-resolv.conf:/etc/resolv.conf'; do
  if [[ "$(grep -Fxc "$required_service_setting" "$DOWNLOADER_SERVICE")" -ne 1 ]]; then
    echo "Downloader-Service fehlt die feste Grenze: $required_service_setting" >&2
    exit 1
  fi
done
if grep -Eq '^(LoadCredential|Environment|EnvironmentFile|PassEnvironment|SupplementaryGroups|ReadWritePaths)=' \
    "$DOWNLOADER_SERVICE"; then
  echo "Downloader-Service erhält Credentials, Konfig-Environment oder zusätzliche Schreibrechte." >&2
  exit 1
fi
if [[ "$(grep -Fxc 'InaccessiblePaths=/run' "$DOWNLOADER_SERVICE")" -ne 1 ]] ||
   ! grep -Fq 'InaccessiblePaths=/run' "$DOWNLOADER_SERVICE"; then
  echo "Downloader-Service kann weiterhin Laufzeitsockets unter /run bzw. /var/run sehen." >&2
  exit 1
fi
if grep -Fq 'RestrictNamespaces=user mnt pid net' "$DOWNLOADER_SERVICE" ||
   grep -Eq '^PrivateNetwork=(yes|true|1)$' "$DOWNLOADER_SERVICE"; then
  echo "Downloader-Service trennt sein Netz entgegen dem cgroup-Egress-Vertrag im Namespace." >&2
  exit 1
fi
for denied_prefix in \
  '0.0.0.0/8' '10.0.0.0/8' '100.64.0.0/10' '127.0.0.0/8' \
  '169.254.0.0/16' '172.16.0.0/12' '192.0.0.0/24' '192.0.2.0/24' \
  '192.31.196.0/24' '192.52.193.0/24' '192.88.99.0/24' \
  '192.168.0.0/16' '192.175.48.0/24' '198.18.0.0/15' \
  '198.51.100.0/24' '203.0.113.0/24' '224.0.0.0/4' '240.0.0.0/4' \
  '::/128' '::1/128' '::/96' '::ffff:0:0/96' \
  '64:ff9b::/96' '64:ff9b:1::/48' '100::/64' '100:0:0:1::/64' \
  '2001::/23' '2001:db8::/32' '2002::/16' '2620:4f:8000::/48' \
  '3fff::/20' '5f00::/16' 'fc00::/7' 'fec0::/10' 'fe80::/10' \
  'ff00::/8'; do
  if [[ "$(grep -Fxc "IPAddressDeny=$denied_prefix" "$DOWNLOADER_SERVICE")" -ne 1 ]]; then
    echo "Downloader-Service sperrt einen privaten/reservierten Netzbereich nicht: $denied_prefix" >&2
    exit 1
  fi
done
if grep -Eq '(^|[[:space:]])(100\.100\.100\.100|fd7a:)' "$DOWNLOADER_RESOLVER" ||
   grep -Eq '^(search|domain|sortlist)[[:space:]]' "$DOWNLOADER_RESOLVER"; then
  echo "Downloader-Resolver enthält private Tailscale-/Suchdomänen-Konfiguration." >&2
  exit 1
fi
for required_firewall_setting in \
  'ExecStartPre=/usr/sbin/nft --check --file /opt/deadlock/twitch/current/ops/systemd/tb-media-downloader-egress.nft' \
  'ExecStartPre=/opt/deadlock/twitch/current/ops/systemd/validate-tb-media-downloader-firewall.sh --before-load' \
  'ExecStart=/usr/sbin/nft --file /opt/deadlock/twitch/current/ops/systemd/tb-media-downloader-egress.nft' \
  'ExecStartPost=/opt/deadlock/twitch/current/ops/systemd/validate-tb-media-downloader-firewall.sh --after-load' \
  'RemainAfterExit=yes'; do
  if [[ "$(grep -Fxc "$required_firewall_setting" "$DOWNLOADER_FIREWALL")" -ne 1 ]]; then
    echo "Downloader-Firewall-Unit fehlt die atomare Ladegrenze: $required_firewall_setting" >&2
    exit 1
  fi
done
for required_refresh_setting in \
  'Type=oneshot' \
  'ExecStartPre=/usr/sbin/nft --check --file /opt/deadlock/twitch/current/ops/systemd/tb-media-downloader-egress.nft' \
  'ExecStartPre=/opt/deadlock/twitch/current/ops/systemd/validate-tb-media-downloader-firewall.sh --before-load' \
  'ExecStart=/usr/sbin/nft --file /opt/deadlock/twitch/current/ops/systemd/tb-media-downloader-egress.nft' \
  'ExecStartPost=/opt/deadlock/twitch/current/ops/systemd/validate-tb-media-downloader-firewall.sh --after-load' \
  'CapabilityBoundingSet=CAP_NET_ADMIN' \
  'RestrictAddressFamilies=AF_NETLINK'; do
  if [[ "$(grep -Fxc "$required_refresh_setting" "$DOWNLOADER_FIREWALL_REFRESH")" -ne 1 ]]; then
    echo "Downloader-Firewall-Refresh ist nicht eng und atomar: $required_refresh_setting" >&2
    exit 1
  fi
done
if grep -q '^RemainAfterExit=' "$DOWNLOADER_FIREWALL_REFRESH" ||
   grep -Eq '%i|%I' <(grep -E '^Exec(Start|Condition|StartPre)=' "$DOWNLOADER_FIREWALL_REFRESH"); then
  echo "Downloader-Firewall-Refresh bleibt stale oder schleust Instanzdaten in Root-Argumente." >&2
  exit 1
fi
for required_nft_fragment in \
  'add table inet deadlock_twitch_downloader' \
  'flush table inet deadlock_twitch_downloader' \
  'type filter hook output priority filter; policy accept;' \
  'meta skuid != "twitchdownload" return' \
  'ip6 daddr { fe80::/10, ff02::/16 } icmpv6 type { nd-router-solicit, nd-router-advert, nd-neighbor-solicit, nd-neighbor-advert } ip6 hoplimit 255 counter accept' \
  'fib daddr type local counter drop comment "twitchdownload: hosteigene Adressen sperren"' \
  'ip daddr { 1.1.1.1, 1.0.0.1 } udp dport 53 counter accept' \
  'ip daddr { 1.1.1.1, 1.0.0.1 } tcp dport 53 counter accept' \
  'ip6 daddr { 2606:4700:4700::1111, 2606:4700:4700::1001 } udp dport 53 counter accept' \
  'ip6 daddr { 2606:4700:4700::1111, 2606:4700:4700::1001 } tcp dport 53 counter accept' \
  'ip daddr 0.0.0.0/0 tcp dport { 80, 443 } counter accept' \
  'ip6 daddr 2000::/3 tcp dport { 80, 443 } counter accept' \
  'counter drop comment "twitchdownload: sonstigen Egress sperren"'; do
  if ! grep -Fq "$required_nft_fragment" "$DOWNLOADER_NFT"; then
    echo "Downloader-nft-Gate fehlt die feste Allowlist: $required_nft_fragment" >&2
    exit 1
  fi
done
for denied_prefix in \
  '0.0.0.0/8' '10.0.0.0/8' '100.64.0.0/10' '127.0.0.0/8' \
  '169.254.0.0/16' '172.16.0.0/12' '192.0.0.0/24' '192.0.2.0/24' \
  '192.31.196.0/24' '192.52.193.0/24' '192.88.99.0/24' \
  '192.168.0.0/16' '192.175.48.0/24' '198.18.0.0/15' \
  '198.51.100.0/24' '203.0.113.0/24' '224.0.0.0/4' '240.0.0.0/4' \
  '::/96' '::ffff:0:0/96' '64:ff9b::/96' '64:ff9b:1::/48' \
  '100::/64' '100:0:0:1::/64' '2001::/23' '2001:db8::/32' \
  '2002::/16' '2620:4f:8000::/48' '3fff::/20' '5f00::/16' \
  'fc00::/7' 'fec0::/10' 'fe80::/10' 'ff00::/8'; do
  if ! grep -Fq "$denied_prefix" "$DOWNLOADER_NFT"; then
    echo "Downloader-nft-Gate sperrt einen privaten/reservierten Bereich nicht: $denied_prefix" >&2
    exit 1
  fi
done
if [[ "$(grep -Ec '^nameserver[[:space:]]+' "$DOWNLOADER_RESOLVER")" -lt 2 ]]; then
  echo "Downloader-Resolver enthält keine redundanten öffentlichen Resolver." >&2
  exit 1
fi
for unit in "$BOT_UNIT" "$DASHBOARD_UNIT"; do
  if [[ "$(grep -c '^RestrictNamespaces=user mnt pid net$' "$unit")" -ne 1 ]] ||
     grep -q '^RestrictNamespaces=yes$' "$unit"; then
    echo "Runtime-Unit erlaubt nicht exakt die für Bubblewrap nötigen Namespaces: $unit" >&2
    exit 1
  fi
  if [[ "$(grep -c '^TasksMax=64$' "$unit")" -ne 1 ]]; then
    echo "Runtime-Unit begrenzt die Dienstprozesse nicht hart: $unit" >&2
    exit 1
  fi
done
for unit_path in \
  ops/systemd/deadlock-twitch-bot-rust.service \
  ops/systemd/deadlock-twitch-dashboard-rust.service; do
  if [[ "$(grep -Fc "$unit_path" "$INSTALLER")" -lt 3 ]]; then
    echo "Systemd-Unit wird nicht geprüft aus dem Git-SHA in den Releasebaum übernommen: $unit_path" >&2
    exit 1
  fi
done
for release_token in \
  DOWNLOADER_RELEASE_PATH \
  DOWNLOADER_SOCKET_PATH \
  DOWNLOADER_SERVICE_PATH \
  DOWNLOADER_FIREWALL_PATH \
  DOWNLOADER_FIREWALL_REFRESH_PATH \
  DOWNLOADER_FIREWALL_VALIDATOR_PATH \
  DOWNLOADER_RESOLVER_PATH \
  DOWNLOADER_NFT_PATH; do
  if [[ "$(grep -Fc "$release_token" "$INSTALLER")" -lt 3 ]]; then
    echo "Downloader-Komponente wird nicht vollständig geprüft und gestuft: $release_token" >&2
    exit 1
  fi
done
for runtime_account in twitchbot twitchdash; do
  if [[ "$(grep -Fc "probe_bwrap_fd_sandbox $runtime_account" "$INSTALLER")" -ne 1 ]]; then
    echo "Installer prüft die FD-Sandbox nicht als $runtime_account." >&2
    exit 1
  fi
done
if [[ "$(grep -c '^install_release_system_unit ' "$INSTALLER")" -ne 6 ]] ||
   [[ "$(grep -Fxc '/usr/bin/systemctl daemon-reload' "$INSTALLER")" -ne 1 ]]; then
  echo "Installer verdrahtet nicht exakt alle sechs systemweiten Units vor dem Deploy-Gate." >&2
  exit 1
fi
for installed_unit in \
  deadlock-twitch-bot-rust.service \
  deadlock-twitch-dashboard-rust.service \
  deadlock-twitch-media-downloader.socket \
  deadlock-twitch-media-downloader@.service \
  deadlock-twitch-media-downloader-firewall.service \
  deadlock-twitch-media-downloader-firewall-refresh@.service; do
  if [[ "$(grep -Fc "$installed_unit" "$INSTALLER")" -lt 2 ]]; then
    echo "Installer übernimmt eine erforderliche System-Unit nicht: $installed_unit" >&2
    exit 1
  fi
done
if grep -Eq '/usr/bin/systemctl[[:space:]]+(start|stop|restart|enable|disable)([[:space:]]|$)' \
    "$INSTALLER"; then
  echo "Installer mutiert entgegen dem Offline-Gate einen Dienst-Lebenszyklus." >&2
  exit 1
fi
last_unit_install_line="$(grep -n '^install_release_system_unit ' "$INSTALLER" | tail -n 1)"
last_unit_install_line="${last_unit_install_line%%:*}"
daemon_reload_line="$(grep -nFx '/usr/bin/systemctl daemon-reload' "$INSTALLER")"
daemon_reload_line="${daemon_reload_line%%:*}"
# shellcheck disable=SC2016
current_switch_line="$(grep -n 'mv -Tf -- "$current_tmp" /opt/deadlock/twitch/current' "$INSTALLER")"
current_switch_line="${current_switch_line%%:*}"
if ((daemon_reload_line <= last_unit_install_line ||
     current_switch_line <= daemon_reload_line)); then
  echo "Installer lädt die vollständige Unitmenge nicht vor dem atomaren Current-Wechsel." >&2
  exit 1
fi
TEST_DIR="$(mktemp -d)"
trap 'find "$TEST_DIR" -xdev -depth -delete' EXIT
SYSTEMD_TEST_ROOT="$TEST_DIR/systemd-root"
install -D -m 0755 /usr/bin/true \
  "$SYSTEMD_TEST_ROOT/opt/deadlock/twitch/current/rust/target/release/tb-media-downloader"
install -D -m 0755 /usr/bin/true "$SYSTEMD_TEST_ROOT/usr/sbin/nft"
install -D -m 0755 "$DOWNLOADER_FIREWALL_VALIDATOR" \
  "$SYSTEMD_TEST_ROOT/opt/deadlock/twitch/current/ops/systemd/validate-tb-media-downloader-firewall.sh"
install -D -m 0644 "$DOWNLOADER_RESOLVER" \
  "$SYSTEMD_TEST_ROOT/opt/deadlock/twitch/current/ops/systemd/tb-media-downloader-resolv.conf"
install -D -m 0644 "$DOWNLOADER_NFT" \
  "$SYSTEMD_TEST_ROOT/opt/deadlock/twitch/current/ops/systemd/tb-media-downloader-egress.nft"
install -D -m 0644 "$DOWNLOADER_SOCKET" \
  "$SYSTEMD_TEST_ROOT/etc/systemd/system/deadlock-twitch-media-downloader.socket"
install -D -m 0644 "$DOWNLOADER_SERVICE" \
  "$SYSTEMD_TEST_ROOT/etc/systemd/system/deadlock-twitch-media-downloader@.service"
install -D -m 0644 "$DOWNLOADER_FIREWALL" \
  "$SYSTEMD_TEST_ROOT/etc/systemd/system/deadlock-twitch-media-downloader-firewall.service"
install -D -m 0644 "$DOWNLOADER_FIREWALL_REFRESH" \
  "$SYSTEMD_TEST_ROOT/etc/systemd/system/deadlock-twitch-media-downloader-firewall-refresh@.service"
systemd-analyze --root="$SYSTEMD_TEST_ROOT" --recursive-errors=no verify \
  deadlock-twitch-media-downloader-firewall.service \
  deadlock-twitch-media-downloader-firewall-refresh@.service \
  deadlock-twitch-media-downloader.socket \
  deadlock-twitch-media-downloader@.service
TEST_CHECKSUM="$TEST_DIR/yt-dlp-linux.sha256"
printf '%064d  yt-dlp_linux\n' 0 >"$TEST_CHECKSUM"

erwarte_fehler() {
  local erwartete_meldung="$1"
  local artefakt="$2"
  local ausgabe
  if ausgabe="$("$INSTALLER" --validate-yt-dlp-artifact "$artefakt" "$TEST_CHECKSUM" 2>&1)"; then
    echo "Installer akzeptiert ungültiges yt-dlp-Artefakt: $artefakt" >&2
    exit 1
  fi
  if [[ "$ausgabe" != *"$erwartete_meldung"* ]]; then
    echo "Unerwartete Fehlermeldung: $ausgabe" >&2
    exit 1
  fi
}

fehlend="$TEST_DIR/fehlend/yt-dlp"
erwarte_fehler "yt-dlp-Release-Artefakt fehlt" "$fehlend"

nicht_ausfuehrbar="$TEST_DIR/nicht-ausfuehrbar"
printf '#!/usr/bin/env bash\nexit 0\n' >"$nicht_ausfuehrbar"
chmod 0644 "$nicht_ausfuehrbar"
erwarte_fehler "yt-dlp-Release-Artefakt ist nicht ausführbar" "$nicht_ausfuehrbar"

verzeichnis="$TEST_DIR/verzeichnis"
mkdir "$verzeichnis"
chmod 0755 "$verzeichnis"
erwarte_fehler "yt-dlp-Release-Artefakt ist keine reguläre Datei" "$verzeichnis"

ziel="$TEST_DIR/ziel"
printf '#!/usr/bin/env bash\nexit 0\n' >"$ziel"
chmod 0755 "$ziel"
symlink="$TEST_DIR/symlink"
ln -s "$ziel" "$symlink"
erwarte_fehler "yt-dlp-Release-Artefakt darf kein Symlink sein" "$symlink"

echtes_verzeichnis="$TEST_DIR/echtes-verzeichnis"
mkdir "$echtes_verzeichnis"
cp "$ziel" "$echtes_verzeichnis/yt-dlp"
symlink_verzeichnis="$TEST_DIR/symlink-verzeichnis"
ln -s "$echtes_verzeichnis" "$symlink_verzeichnis"
erwarte_fehler "Release-Artefaktpfad enthält einen Symlink" "$symlink_verzeichnis/yt-dlp"

binaer="$TEST_DIR/yt-dlp"
printf '#!/usr/bin/env bash\nexit 0\n' >"$binaer"
chmod 0755 "$binaer"
erwarte_fehler "yt-dlp-Prüfsumme stimmt nicht" "$binaer"
binaer_sha="$(sha256sum -- "$binaer")"
printf '%s  yt-dlp_linux\n' "${binaer_sha%% *}" >"$TEST_CHECKSUM"
# Die Funktion wird absichtlich nur in den Kindprozess exportiert.
# shellcheck disable=SC2317
sha256sum() {
  echo "Eine geerbte Shell-Funktion wurde ausgeführt." >&2
  return 93
}
# shellcheck disable=SC2317
compgen() {
  return 0
}
export -f sha256sum compgen
BASH_ENV_DATEI="$TEST_DIR/feindliche-bash-env"
BASH_ENV_MARKER="$TEST_DIR/bash-env-wurde-geladen"
printf 'printf geladen >%q\n' "$BASH_ENV_MARKER" >"$BASH_ENV_DATEI"
BEREINIGTE_AUSGABE="$TEST_DIR/bereinigte-umgebung-ausgabe"
if ! BASH_ENV="$BASH_ENV_DATEI" \
  LD_PRELOAD="$TEST_DIR/nicht-vorhandenes-preload.so" \
  LD_LIBRARY_PATH="$TEST_DIR/nicht-vertrauenswuerdig" \
  TAR_OPTIONS='--help' GIT_EXEC_PATH="$TEST_DIR/nicht-vertrauenswuerdig" \
  "$INSTALLER" --validate-yt-dlp-artifact "$binaer" "$TEST_CHECKSUM" \
  >"$BEREINIGTE_AUSGABE" 2>&1; then
  echo "Root-Installer bereinigt eine feindliche Umgebung nicht vor Bash." >&2
  sed -n '1,20p' "$BEREINIGTE_AUSGABE" >&2
  exit 1
fi
if [[ -e "$BASH_ENV_MARKER" ]]; then
  echo "Root-Installer hat eine geerbte BASH_ENV vor der Bereinigung geladen." >&2
  exit 1
fi
if BASH_UMGEHUNG_AUSGABE="$(
  LD_LIBRARY_PATH="$TEST_DIR/nicht-vertrauenswuerdig" \
    /usr/bin/bash --noprofile --norc "$INSTALLER" \
      --validate-yt-dlp-artifact "$binaer" "$TEST_CHECKSUM" 2>&1
)"; then
  echo "Root-Installer akzeptiert das Umgehen seiner bereinigenden Shebang." >&2
  exit 1
fi
if [[ "$BASH_UMGEHUNG_AUSGABE" != *"vor Bash bereinigten Umgebung"* ]]; then
  echo "Root-Installer scheitert bei umgangener Shebang nicht verständlich." >&2
  exit 1
fi
unset -f sha256sum compgen

TEST_UID="$(id -u)"
TEST_GID="$(id -g)"

firewall_json_valid="$TEST_DIR/firewall-valid.json"
{
  printf '{"nftables":[{"metainfo":{}},'
  printf '{"table":{"family":"inet","name":"deadlock_twitch_downloader","handle":1}},'
  printf '{"chain":{"family":"inet","table":"deadlock_twitch_downloader","name":"output","handle":2,"type":"filter","hook":"output","prio":0,"policy":"accept"}}'
  for json_rule_index in 1 2 3 4 5 6 7 8 9 10 11 12; do
    printf ',{"rule":{"family":"inet","table":"deadlock_twitch_downloader","chain":"output","handle":%d}}' \
      "$((json_rule_index + 2))"
  done
  printf ']}\n'
} >"$firewall_json_valid"
chmod 0644 "$firewall_json_valid"
"$DOWNLOADER_FIREWALL_VALIDATOR" --validate-json-for-test \
  before "$firewall_json_valid"
"$DOWNLOADER_FIREWALL_VALIDATOR" --validate-json-for-test \
  after "$firewall_json_valid"

firewall_json_flushed="$TEST_DIR/firewall-flushed.json"
printf '%s\n' \
  '{"nftables":[{"metainfo":{}},{"table":{"family":"inet","name":"deadlock_twitch_downloader","handle":1}},{"chain":{"family":"inet","table":"deadlock_twitch_downloader","name":"output","handle":2,"type":"filter","hook":"output","prio":0,"policy":"accept"}}]}' \
  >"$firewall_json_flushed"
chmod 0644 "$firewall_json_flushed"
"$DOWNLOADER_FIREWALL_VALIDATOR" --validate-json-for-test \
  before "$firewall_json_flushed"
if "$DOWNLOADER_FIREWALL_VALIDATOR" --validate-json-for-test \
    after "$firewall_json_flushed" >/dev/null 2>&1; then
  echo "Firewall-Liveprüfung akzeptiert nach dem Laden eine leere Regelmenge." >&2
  exit 1
fi

firewall_json_extra_chain="$TEST_DIR/firewall-extra-chain.json"
printf '%s\n' \
  '{"nftables":[{"metainfo":{}},{"table":{"family":"inet","name":"deadlock_twitch_downloader","handle":1}},{"chain":{"family":"inet","table":"deadlock_twitch_downloader","name":"output","handle":2,"type":"filter","hook":"output","prio":0,"policy":"accept"}},{"chain":{"family":"inet","table":"deadlock_twitch_downloader","name":"unerwartet","handle":99,"type":"filter","hook":"output","prio":10,"policy":"accept"}}]}' \
  >"$firewall_json_extra_chain"
chmod 0644 "$firewall_json_extra_chain"
if "$DOWNLOADER_FIREWALL_VALIDATOR" --validate-json-for-test \
    before "$firewall_json_extra_chain" >/dev/null 2>&1; then
  echo "Firewall-Vorprüfung akzeptiert eine vorplatzierte fremde Base-Chain." >&2
  exit 1
fi

firewall_json_extra_set="$TEST_DIR/firewall-extra-set.json"
printf '%s\n' \
  '{"nftables":[{"metainfo":{}},{"table":{"family":"inet","name":"deadlock_twitch_downloader","handle":1}},{"chain":{"family":"inet","table":"deadlock_twitch_downloader","name":"output","handle":2,"type":"filter","hook":"output","prio":0,"policy":"accept"}},{"set":{"family":"inet","table":"deadlock_twitch_downloader","name":"unerwartet","type":"ipv4_addr"}}]}' \
  >"$firewall_json_extra_set"
chmod 0644 "$firewall_json_extra_set"
if "$DOWNLOADER_FIREWALL_VALIDATOR" --validate-json-for-test \
    before "$firewall_json_extra_set" >/dev/null 2>&1; then
  echo "Firewall-Vorprüfung akzeptiert ein vorplatziertes Set/Map/Object." >&2
  exit 1
fi

unit_release="$TEST_DIR/unit-release"
unit_source_relative='ops/systemd/test-atomic.service'
unit_source="$unit_release/$unit_source_relative"
unit_target_dir="$TEST_DIR/unit-target"
unit_target="$unit_target_dir/test-atomic.service"
mkdir -p "$(dirname "$unit_source")" "$unit_target_dir"
chmod 0755 "$unit_release" "$unit_release/ops" \
  "$unit_release/ops/systemd" "$unit_target_dir"
printf '[Service]\nExecStart=/usr/bin/true\n' >"$unit_source"
chmod 0644 "$unit_source"
"$INSTALLER" --install-system-unit-for-test \
  "$unit_release" "$unit_source_relative" test-atomic.service \
  "$unit_target_dir" "$TEST_UID" "$TEST_GID"
if ! cmp --silent -- "$unit_source" "$unit_target" ||
   [[ "$(stat -c '%u:%g:%h:%a' -- "$unit_target")" != \
      "$TEST_UID:$TEST_GID:1:644" ]]; then
  echo "Atomare Unitinstallation erzeugt nicht das erwartete reguläre 0644-Ziel." >&2
  exit 1
fi
printf '[Service]\nExecStart=/usr/bin/false\n' >"$unit_source"
"$INSTALLER" --install-system-unit-for-test \
  "$unit_release" "$unit_source_relative" test-atomic.service \
  "$unit_target_dir" "$TEST_UID" "$TEST_GID"
if ! cmp --silent -- "$unit_source" "$unit_target" ||
   [[ -n "$(find "$unit_target_dir" -maxdepth 1 \
      -name '.test-atomic.service.install-*' -print -quit)" ]]; then
  echo "Atomare Unitaktualisierung ersetzt das Ziel nicht sauber." >&2
  exit 1
fi

unit_external="$TEST_DIR/unit-external"
printf 'unverändert\n' >"$unit_external"
rm "$unit_target"
ln -s "$unit_external" "$unit_target"
if "$INSTALLER" --install-system-unit-for-test \
    "$unit_release" "$unit_source_relative" test-atomic.service \
    "$unit_target_dir" "$TEST_UID" "$TEST_GID" >/dev/null 2>&1; then
  echo "Atomare Unitinstallation akzeptiert ein bestehendes Symlink-Ziel." >&2
  exit 1
fi
if [[ "$(<"$unit_external")" != 'unverändert' ]]; then
  echo "Atomare Unitinstallation hat ein externes Symlink-Ziel verändert." >&2
  exit 1
fi
rm "$unit_target"
ln "$unit_external" "$unit_target"
if "$INSTALLER" --install-system-unit-for-test \
    "$unit_release" "$unit_source_relative" test-atomic.service \
    "$unit_target_dir" "$TEST_UID" "$TEST_GID" >/dev/null 2>&1; then
  echo "Atomare Unitinstallation akzeptiert ein mehrfach verlinktes Ziel." >&2
  exit 1
fi
rm "$unit_target"
unit_source_link="$unit_release/ops/systemd/test-source-link.service"
ln -s "$unit_source" "$unit_source_link"
if "$INSTALLER" --install-system-unit-for-test \
    "$unit_release" ops/systemd/test-source-link.service test-atomic.service \
    "$unit_target_dir" "$TEST_UID" "$TEST_GID" >/dev/null 2>&1; then
  echo "Atomare Unitinstallation akzeptiert eine Symlink-Quelle." >&2
  exit 1
fi
if "$INSTALLER" --install-system-unit-for-test \
    "$unit_release" "$unit_source_relative" ../escaped.service \
    "$unit_target_dir" "$TEST_UID" "$TEST_GID" >/dev/null 2>&1; then
  echo "Atomare Unitinstallation akzeptiert einen Unitnamen mit Pfadanteil." >&2
  exit 1
fi

nft_test="$TEST_DIR/nft"
# ${1:-} gehört absichtlich in das Testartefakt.
# shellcheck disable=SC2016
printf '#!/usr/bin/bash\ncase "${1:-}" in\n  --version) echo "nftables v1.0.9 (Old Doc Yak #3)" ;;\n  --help) echo "--check --file" ;;\n  describe) [[ "${2:-} ${3:-}" == "meta skuid" ]] && echo "meta skuid" ;;\nesac\n' \
  >"$nft_test"
chmod 0755 "$nft_test"
"$INSTALLER" --validate-nft-for-test "$nft_test" 1.0.9 "$TEST_UID" "$TEST_GID"
"$INSTALLER" --validate-nft-for-test "$nft_test" 1.0.8 "$TEST_UID" "$TEST_GID"
nft_old="$TEST_DIR/nft-old"
sed 's/v1\.0\.9/v1.0.8/' "$nft_test" >"$nft_old"
chmod 0755 "$nft_old"
if nft_ausgabe="$("$INSTALLER" --validate-nft-for-test \
    "$nft_old" 1.0.9 "$TEST_UID" "$TEST_GID" 2>&1)"; then
  echo "Installer akzeptiert ein zu altes nftables." >&2
  exit 1
fi
if [[ "$nft_ausgabe" != *"nftables ist zu alt"* ]]; then
  echo "Installer meldet ein zu altes nftables unklar: $nft_ausgabe" >&2
  exit 1
fi
chmod 0775 "$nft_test"
if "$INSTALLER" --validate-nft-for-test \
    "$nft_test" 1.0.9 "$TEST_UID" "$TEST_GID" >/dev/null 2>&1; then
  echo "Installer akzeptiert ein gruppenschreibbares nftables-Werkzeug." >&2
  exit 1
fi
chmod 0755 "$nft_test"

resolver_test="$TEST_DIR/downloader-resolv.conf"
cp "$DOWNLOADER_RESOLVER" "$resolver_test"
chmod 0644 "$resolver_test"
"$INSTALLER" --validate-public-resolver-for-test "$resolver_test" "$TEST_UID" "$TEST_GID"
resolver_private="$TEST_DIR/downloader-resolv-private.conf"
printf 'nameserver 100.100.100.100\nnameserver 1.1.1.1\noptions timeout:2 attempts:2 rotate edns0\n' \
  >"$resolver_private"
chmod 0644 "$resolver_private"
if resolver_ausgabe="$("$INSTALLER" --validate-public-resolver-for-test \
    "$resolver_private" "$TEST_UID" "$TEST_GID" 2>&1)"; then
  echo "Installer akzeptiert den privaten Tailscale-Resolver." >&2
  exit 1
fi
if [[ "$resolver_ausgabe" != *"öffentliche Resolverdatei"* ]]; then
  echo "Installer meldet den privaten Resolver unklar: $resolver_ausgabe" >&2
  exit 1
fi
chmod 0664 "$resolver_test"
if "$INSTALLER" --validate-public-resolver-for-test \
    "$resolver_test" "$TEST_UID" "$TEST_GID" >/dev/null 2>&1; then
  echo "Installer akzeptiert eine gruppenschreibbare Resolverdatei." >&2
  exit 1
fi
chmod 0644 "$resolver_test"
resolver_link="$TEST_DIR/downloader-resolv-link.conf"
ln -s "$resolver_test" "$resolver_link"
if "$INSTALLER" --validate-public-resolver-for-test \
    "$resolver_link" "$TEST_UID" "$TEST_GID" >/dev/null 2>&1; then
  echo "Installer akzeptiert eine Resolverdatei als Symlink." >&2
  exit 1
fi

downloader_passwd='twitchdownload:x:990:989::/nonexistent:/usr/sbin/nologin'
downloader_group='twitchdownload:x:989:'
downloader_all_passwd=$'twitchbot:x:995:984::/var/empty:/usr/sbin/nologin\ntwitchdash:x:991:983::/var/empty:/usr/sbin/nologin\ntwitchdownload:x:990:989::/nonexistent:/usr/sbin/nologin'
"$INSTALLER" --validate-downloader-account-for-test \
  "$downloader_passwd" "$downloader_group" "$downloader_all_passwd" '989'
if "$INSTALLER" --validate-downloader-account-for-test \
    "$downloader_passwd" "$downloader_group" "$downloader_all_passwd" '989 985' \
    >/dev/null 2>&1; then
  echo "Installer akzeptiert den Downloader in einer zusätzlichen Mediengruppe." >&2
  exit 1
fi
if "$INSTALLER" --validate-downloader-account-for-test \
    'twitchdownload:x:995:989::/nonexistent:/usr/sbin/nologin' \
    "$downloader_group" "$downloader_all_passwd" '989' >/dev/null 2>&1; then
  echo "Installer akzeptiert eine mit dem Bot geteilte Downloader-UID." >&2
  exit 1
fi
if "$INSTALLER" --validate-downloader-account-for-test \
    'twitchdownload:x:990:989::/var/lib/deadlock-twitch-media:/usr/bin/bash' \
    "$downloader_group" "$downloader_all_passwd" '989' >/dev/null 2>&1; then
  echo "Installer akzeptiert Downloader-Login oder Medien-Home." >&2
  exit 1
fi
if "$INSTALLER" --validate-downloader-account-for-test \
    "$downloader_passwd" 'twitchdownload:x:989:twitchdownload' \
    "$downloader_all_passwd" '989' >/dev/null 2>&1; then
  echo "Installer akzeptiert explizite Downloader-Gruppenmitglieder." >&2
  exit 1
fi

bwrap_test="$TEST_DIR/bwrap"
# ${1:-} gehört absichtlich in das Testartefakt.
# shellcheck disable=SC2016
printf '#!/usr/bin/bash\ncase "${1:-}" in\n  --version) echo "bubblewrap 0.9.0" ;;\n  --help) echo "--ro-bind-fd --bind-fd --file --unshare-user --unshare-pid --unshare-net --disable-userns --cap-drop --clearenv --die-with-parent" ;;\nesac\n' >"$bwrap_test"
chmod 0755 "$bwrap_test"
"$INSTALLER" --validate-bwrap-for-test "$bwrap_test" 0.9.0 "$TEST_UID" "$TEST_GID"
"$INSTALLER" --validate-bwrap-for-test "$bwrap_test" 0.8.0 "$TEST_UID" "$TEST_GID"

bwrap_alt="$TEST_DIR/bwrap-alt"
# ${1:-} gehört absichtlich in das Testartefakt.
# shellcheck disable=SC2016
printf '#!/usr/bin/bash\ncase "${1:-}" in\n  --version) echo "bubblewrap 0.10.1" ;;\n  --help) echo "--ro-bind-fd --bind-fd --file --unshare-user --unshare-pid --unshare-net --disable-userns --cap-drop --clearenv --die-with-parent" ;;\nesac\n' >"$bwrap_alt"
chmod 0755 "$bwrap_alt"
"$INSTALLER" --validate-bwrap-for-test "$bwrap_alt" 0.9.0 "$TEST_UID" "$TEST_GID"

bwrap_old="$TEST_DIR/bwrap-old"
# ${1:-} gehört absichtlich in das Testartefakt.
# shellcheck disable=SC2016
printf '#!/usr/bin/bash\ncase "${1:-}" in\n  --version) echo "bubblewrap 0.8.0" ;;\n  --help) echo "--ro-bind-fd --bind-fd --file --unshare-user --unshare-pid --unshare-net --disable-userns --cap-drop --clearenv --die-with-parent" ;;\nesac\n' >"$bwrap_old"
chmod 0755 "$bwrap_old"
if bwrap_ausgabe="$("$INSTALLER" --validate-bwrap-for-test \
    "$bwrap_old" 0.9.0 "$TEST_UID" "$TEST_GID" 2>&1)"; then
  echo "Installer akzeptiert ein zu altes Bubblewrap." >&2
  exit 1
fi
if [[ "$bwrap_ausgabe" != *"Bubblewrap ist zu alt"* ]]; then
  echo "Installer meldet ein zu altes Bubblewrap unklar: $bwrap_ausgabe" >&2
  exit 1
fi

bwrap_featureless="$TEST_DIR/bwrap-featureless"
# ${1:-} gehört absichtlich in das Testartefakt.
# shellcheck disable=SC2016
printf '#!/usr/bin/bash\ncase "${1:-}" in\n  --version) echo "bubblewrap 0.9.0" ;;\n  --help) echo "--unshare-user --unshare-pid --unshare-net --disable-userns --cap-drop --clearenv --die-with-parent" ;;\nesac\n' >"$bwrap_featureless"
chmod 0755 "$bwrap_featureless"
if bwrap_ausgabe="$("$INSTALLER" --validate-bwrap-for-test \
    "$bwrap_featureless" 0.9.0 "$TEST_UID" "$TEST_GID" 2>&1)"; then
  echo "Installer akzeptiert Bubblewrap ohne FD-Bind-Unterstützung." >&2
  exit 1
fi
if [[ "$bwrap_ausgabe" != *"benötigte Option nicht: --ro-bind-fd"* ]]; then
  echo "Installer meldet fehlende Bubblewrap-Funktion unklar: $bwrap_ausgabe" >&2
  exit 1
fi

bwrap_ohne_file="$TEST_DIR/bwrap-ohne-file"
sed 's/ --file//' "$bwrap_test" >"$bwrap_ohne_file"
chmod 0755 "$bwrap_ohne_file"
if bwrap_ausgabe="$("$INSTALLER" --validate-bwrap-for-test \
    "$bwrap_ohne_file" 0.9.0 "$TEST_UID" "$TEST_GID" 2>&1)"; then
  echo "Installer akzeptiert Bubblewrap ohne Resolver-FD-Datei-Unterstützung." >&2
  exit 1
fi
if [[ "$bwrap_ausgabe" != *"benötigte Option nicht: --file"* ]]; then
  echo "Installer meldet fehlendes Bubblewrap --file unklar: $bwrap_ausgabe" >&2
  exit 1
fi

bwrap_writable="$TEST_DIR/bwrap-writable"
cp "$bwrap_test" "$bwrap_writable"
chmod 0775 "$bwrap_writable"
if bwrap_ausgabe="$("$INSTALLER" --validate-bwrap-for-test \
    "$bwrap_writable" 0.9.0 "$TEST_UID" "$TEST_GID" 2>&1)"; then
  echo "Installer akzeptiert ein gruppenschreibbares Bubblewrap." >&2
  exit 1
fi
if [[ "$bwrap_ausgabe" != *"unsichere Eigentümer-, Link- oder Modusdaten"* ]]; then
  echo "Installer meldet unsichere Bubblewrap-Rechte unklar: $bwrap_ausgabe" >&2
  exit 1
fi

bwrap_hardlink="$TEST_DIR/bwrap-hardlink"
ln "$bwrap_test" "$bwrap_hardlink"
if bwrap_ausgabe="$("$INSTALLER" --validate-bwrap-for-test \
    "$bwrap_test" 0.9.0 "$TEST_UID" "$TEST_GID" 2>&1)"; then
  echo "Installer akzeptiert ein mehrfach verlinktes Bubblewrap." >&2
  exit 1
fi
if [[ "$bwrap_ausgabe" != *"unsichere Eigentümer-, Link- oder Modusdaten"* ]]; then
  echo "Installer meldet Bubblewrap-Hardlink unklar: $bwrap_ausgabe" >&2
  exit 1
fi
find "$bwrap_hardlink" -maxdepth 0 -delete

bwrap_symlink="$TEST_DIR/bwrap-symlink"
ln -s "$bwrap_test" "$bwrap_symlink"
if bwrap_ausgabe="$("$INSTALLER" --validate-bwrap-for-test \
    "$bwrap_symlink" 0.9.0 "$TEST_UID" "$TEST_GID" 2>&1)"; then
  echo "Installer akzeptiert einen Bubblewrap-Symlink." >&2
  exit 1
fi
if [[ "$bwrap_ausgabe" != *"fehlt oder ist ein Symlink"* ]]; then
  echo "Installer meldet Bubblewrap-Symlink unklar: $bwrap_ausgabe" >&2
  exit 1
fi

"$INSTALLER" --validate-system-bwrap-for-test

bwrap_probe_dir="$TEST_DIR/bwrap-fd-probe"
bwrap_probe_input="$bwrap_probe_dir/input"
bwrap_probe_foreign="$bwrap_probe_dir/foreign"
bwrap_probe_output="$bwrap_probe_dir/output"
bwrap_probe_resolver="$bwrap_probe_dir/resolv.conf"
mkdir -m 0700 "$bwrap_probe_dir"
mkdir -m 0700 "$bwrap_probe_output"
printf 'bubblewrap-fd-probe\n' >"$bwrap_probe_input"
printf 'darf-nicht-in-die-sandbox\n' >"$bwrap_probe_foreign"
printf 'nameserver 1.1.1.1\n' >"$bwrap_probe_resolver"
chmod 0640 "$bwrap_probe_input" "$bwrap_probe_foreign" "$bwrap_probe_resolver"
(
  exec 7<"$bwrap_probe_foreign"
  exec 8<"$bwrap_probe_input"
  exec 9<"$bwrap_probe_output"
  exec 10<"$bwrap_probe_resolver"
  for fd_path in /proc/self/fd/*; do
    fd="${fd_path##*/}"
    if [[ "$fd" =~ ^[0-9]+$ && "$fd" -gt 2 &&
          "$fd" -ne 8 && "$fd" -ne 9 && "$fd" -ne 10 ]]; then
      eval "exec ${fd}<&-"
    fi
  done
  # Die Variablen werden absichtlich erst von der Sandbox-Bash ausgewertet.
  # shellcheck disable=SC2016
  /usr/bin/bwrap \
    --unshare-user --unshare-pid --unshare-net \
    --new-session --die-with-parent \
    --uid 0 --gid 0 --cap-drop ALL --disable-userns --clearenv \
    --dir /usr --dir /usr/bin --dir /usr/lib \
    --dir /usr/lib/x86_64-linux-gnu --dir /usr/lib64 \
    --dir /etc --dir /etc/alternatives --dir /output \
    --ro-bind /usr/bin/bash /usr/bin/bash \
    --ro-bind /usr/lib/x86_64-linux-gnu /usr/lib/x86_64-linux-gnu \
    --ro-bind /usr/lib64 /usr/lib64 \
    --symlink usr/lib /lib --symlink usr/lib64 /lib64 \
    --ro-bind /etc/ld.so.cache /etc/ld.so.cache \
    --ro-bind /etc/alternatives/libblas.so.3-x86_64-linux-gnu \
              /etc/alternatives/libblas.so.3-x86_64-linux-gnu \
    --ro-bind /etc/alternatives/liblapack.so.3-x86_64-linux-gnu \
              /etc/alternatives/liblapack.so.3-x86_64-linux-gnu \
    --dev /dev --tmpfs /tmp \
    --file 10 /etc/resolv.conf \
    --ro-bind-fd 8 /input --bind-fd 9 /output \
    --setenv LC_ALL C -- /usr/bin/bash --noprofile --norc -c '
      if { : <&7; } 2>/dev/null; then
        exit 97
      fi
      IFS= read -r resolver_value </etc/resolv.conf
      [[ "$resolver_value" == "nameserver 1.1.1.1" ]]
      IFS= read -r probe_value </input
      printf "%s\n" "$probe_value" >/output/result
    '
)
if [[ ! -f "$bwrap_probe_output/result" ||
      -L "$bwrap_probe_output/result" ]] ||
   ! cmp --silent -- "$bwrap_probe_input" "$bwrap_probe_output/result"; then
  echo "Unprivilegierte Bubblewrap-FD-Funktionsprobe ist fehlgeschlagen." >&2
  exit 1
fi

media_parent="$TEST_DIR/media-valid"
mkdir -p \
  "$media_parent/clips/.preparation-work" \
  "$media_parent/clips/.retention-quarantine" \
  "$media_parent/clips/rendered/alt" \
  "$media_parent/clips/uploads/nani" \
  "$media_parent/clips/uploads/.dashboard-work"
printf source >"$media_parent/clips/1.mp4"
printf render >"$media_parent/clips/rendered/alt/1-preview.mp4"
manual_upload="$media_parent/clips/uploads/nani/manual:11111111-2222-4333-8444-555555555555.mp4"
printf upload >"$manual_upload"
preparation_source="$media_parent/clips/.preparation-work/41-01234567-89ab-4cde-8fab-0123456789ab-source.tmp.mp4"
preparation_render="$media_parent/clips/.preparation-work/41-01234567-89ab-4cde-8fab-0123456789ab-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-render.tmp.mp4"
retention_source="$media_parent/clips/.retention-quarantine/41-source-01234567-89ab-4cde-8fab-0123456789ab.mp4"
retention_render="$media_parent/clips/.retention-quarantine/41-render-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb-01234567-89ab-4cde-8fab-0123456789ab.mp4"
printf preparation-source >"$preparation_source"
printf preparation-render >"$preparation_render"
printf retention-source >"$retention_source"
printf retention-render >"$retention_render"
dashboard_orphan="$media_parent/clips/uploads/.dashboard-work/.upload-0123456789abcdef0123456789abcdef.tmp.mp4"
dashboard_orphan_final="$media_parent/clips/uploads/nani/manual:aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee.mp4"
printf orphan >"$dashboard_orphan"
ln "$dashboard_orphan" "$dashboard_orphan_final"
chmod 0777 \
  "$media_parent" \
  "$media_parent/clips" \
  "$media_parent/clips/.preparation-work" \
  "$media_parent/clips/.retention-quarantine" \
  "$media_parent/clips/rendered" \
  "$media_parent/clips/rendered/alt" \
  "$media_parent/clips/uploads" \
  "$media_parent/clips/uploads/nani" \
  "$media_parent/clips/uploads/.dashboard-work"
"$INSTALLER" --harden-media-tree-for-test "$media_parent" "$TEST_UID" "$TEST_GID"
for mode_path in \
  "750:$media_parent" \
  "2750:$media_parent/clips" \
  "700:$media_parent/clips/.preparation-work" \
  "700:$media_parent/clips/.retention-quarantine" \
  "2750:$media_parent/clips/rendered" \
  "2750:$media_parent/clips/rendered/alt" \
  "2770:$media_parent/clips/uploads" \
  "2770:$media_parent/clips/uploads/nani" \
  "700:$media_parent/clips/uploads/.dashboard-work" \
  "640:$media_parent/clips/1.mp4" \
  "640:$preparation_source" \
  "640:$preparation_render" \
  "640:$retention_source" \
  "640:$retention_render" \
  "640:$media_parent/clips/rendered/alt/1-preview.mp4" \
  "640:$manual_upload" \
  "640:$dashboard_orphan" \
  "640:$dashboard_orphan_final"; do
  expected_mode="${mode_path%%:*}"
  checked_path="${mode_path#*:}"
  if [[ "$(stat -c '%a' -- "$checked_path")" != "$expected_mode" ]]; then
    echo "Medienrechte sind unerwartet: $checked_path" >&2
    exit 1
  fi
  if [[ "$(stat -c '%u:%g' -- "$checked_path")" != "$TEST_UID:$TEST_GID" ]]; then
    echo "Medieneigentümer sind unerwartet: $checked_path" >&2
    exit 1
  fi
done
if [[ "$(stat -c '%d:%i:%h' -- "$dashboard_orphan")" != \
      "$(stat -c '%d:%i:%h' -- "$dashboard_orphan_final")" ]] ||
   [[ "$(stat -c '%h' -- "$dashboard_orphan")" -ne 2 ]]; then
  echo "Ambiger Dashboard-Upload-Orphan wurde beim Rechtecutover verändert oder gelöscht." >&2
  exit 1
fi
if sudo -n true 2>/dev/null; then
  root_test_parent="$TEST_DIR/media-root-testmodus"
  mkdir -p "$root_test_parent/clips"
  root_test_mode_before="$(stat -c '%a' -- "$root_test_parent")"
  if root_test_output="$(sudo -n "$INSTALLER" --harden-media-tree-for-test \
      "$root_test_parent" "$TEST_UID" "$TEST_GID" 2>&1)"; then
    echo "Root-Installer exponiert den frei wählbaren Medien-Testmodus als root." >&2
    exit 1
  fi
  if [[ "$root_test_output" != *"darf niemals mit Root-Rechten laufen"* ]] ||
     [[ "$(stat -c '%a' -- "$root_test_parent")" != "$root_test_mode_before" ]]; then
    echo "Root-Medien-Testmodus scheitert nicht unverändert und verständlich." >&2
    exit 1
  fi
fi

erwarte_medien_fehler() {
  local erwartete_meldung="$1"
  local parent="$2"
  local ausgabe
  if ausgabe="$("$INSTALLER" --harden-media-tree-for-test \
      "$parent" "$TEST_UID" "$TEST_GID" 2>&1)"; then
    echo "Installer akzeptiert einen unsicheren Medienbaum: $parent" >&2
    exit 1
  fi
  if [[ "$ausgabe" != *"$erwartete_meldung"* ]]; then
    echo "Unerwartete Medienbaum-Fehlermeldung: $ausgabe" >&2
    exit 1
  fi
}

media_symlink="$TEST_DIR/media-symlink"
mkdir -p "$media_symlink/clips" "$TEST_DIR/media-aussen"
ln -s "$TEST_DIR/media-aussen" "$media_symlink/clips/rendered"
erwarte_medien_fehler "Symlink oder Sonderdateityp" "$media_symlink"

media_fifo="$TEST_DIR/media-fifo"
mkdir -p "$media_fifo/clips"
mkfifo "$media_fifo/clips/fifo"
erwarte_medien_fehler "Symlink oder Sonderdateityp" "$media_fifo"

media_hardlink="$TEST_DIR/media-hardlink"
mkdir -p "$media_hardlink/clips"
printf quelle >"$media_hardlink/clips/quelle.mp4"
ln "$media_hardlink/clips/quelle.mp4" "$media_hardlink/clips/zweite.mp4"
erwarte_medien_fehler "mehrfach verlinkte Datei" "$media_hardlink"

media_unknown="$TEST_DIR/media-unbekannt"
mkdir -p "$media_unknown/clips/fremd"
erwarte_medien_fehler "unerwartetes Unterverzeichnis" "$media_unknown"

media_hidden="$TEST_DIR/media-verstecktes-verzeichnis"
mkdir -p "$media_hidden/clips/uploads/.beliebig"
erwarte_medien_fehler "unerwartetes Unterverzeichnis" "$media_hidden"

media_work_subdir="$TEST_DIR/media-work-unterverzeichnis"
mkdir -p "$media_work_subdir/clips/uploads/.dashboard-work/unterordner"
erwarte_medien_fehler "unerwartetes Unterverzeichnis" "$media_work_subdir"

media_work_file="$TEST_DIR/media-work-fremddatei"
mkdir -p "$media_work_file/clips/uploads/.dashboard-work"
printf fremd >"$media_work_file/clips/uploads/.dashboard-work/fremd.mp4"
erwarte_medien_fehler "Dashboard-Arbeitsordner enthält eine unerwartete Datei" "$media_work_file"

media_client_upload="$TEST_DIR/media-client-uploadname"
mkdir -p "$media_client_upload/clips/uploads/nani"
printf fremd >"$media_client_upload/clips/uploads/nani/client-id.mp4"
erwarte_medien_fehler "keine servergenerierte Manual-Upload-Datei" "$media_client_upload"

media_nested_upload="$TEST_DIR/media-upload-unterverzeichnis"
mkdir -p "$media_nested_upload/clips/uploads/nani/verschachtelt"
erwarte_medien_fehler "unerwartetes Unterverzeichnis" "$media_nested_upload"

media_uppercase_streamer="$TEST_DIR/media-upload-grossbuchstaben"
mkdir -p "$media_uppercase_streamer/clips/uploads/Nani"
printf fremd \
  >"$media_uppercase_streamer/clips/uploads/Nani/manual:11111111-2222-4333-8444-555555555555.mp4"
erwarte_medien_fehler "unerwartetes Unterverzeichnis" "$media_uppercase_streamer"

media_preparation_subdir="$TEST_DIR/media-preparation-unterverzeichnis"
mkdir -p "$media_preparation_subdir/clips/.preparation-work/unterordner"
erwarte_medien_fehler "unerwartetes Unterverzeichnis" "$media_preparation_subdir"

media_preparation_file="$TEST_DIR/media-preparation-fremddatei"
mkdir -p "$media_preparation_file/clips/.preparation-work"
printf fremd >"$media_preparation_file/clips/.preparation-work/41-source.tmp.mp4"
erwarte_medien_fehler "Preparation-Arbeitsordner enthält eine unerwartete Datei" "$media_preparation_file"

media_retention_subdir="$TEST_DIR/media-retention-unterverzeichnis"
mkdir -p "$media_retention_subdir/clips/.retention-quarantine/unterordner"
erwarte_medien_fehler "unerwartetes Unterverzeichnis" "$media_retention_subdir"

media_retention_file="$TEST_DIR/media-retention-fremddatei"
mkdir -p "$media_retention_file/clips/.retention-quarantine"
printf fremd >"$media_retention_file/clips/.retention-quarantine/41-render-01234567-89ab-4cde-8fab-0123456789ab.mp4"
erwarte_medien_fehler "Retention-Quarantäne enthält eine unerwartete Datei" "$media_retention_file"

media_retention_link="$TEST_DIR/media-retention-hardlink"
mkdir -p "$media_retention_link/clips/.retention-quarantine"
printf fremd >"$media_retention_link/ausserhalb.mp4"
ln "$media_retention_link/ausserhalb.mp4" \
  "$media_retention_link/clips/.retention-quarantine/41-source-01234567-89ab-4cde-8fab-0123456789ab.mp4"
erwarte_medien_fehler "unerlaubt mehrfach verlinkte Datei" "$media_retention_link"

media_external_link="$TEST_DIR/media-externer-hardlink"
mkdir -p "$media_external_link/clips/uploads/.dashboard-work"
printf extern >"$media_external_link/ausserhalb.mp4"
ln "$media_external_link/ausserhalb.mp4" \
  "$media_external_link/clips/uploads/.dashboard-work/.upload-abcdefabcdefabcdefabcdefabcdefab.tmp.mp4"
erwarte_medien_fehler "unerlaubt mehrfach verlinkte Datei" "$media_external_link"

test_passwd=$'twitchbot:x:995:984::/var/empty:/usr/sbin/nologin\ntwitchdash:x:991:983::/var/empty:/usr/sbin/nologin'
"$INSTALLER" --validate-media-group-for-test \
  'twitchmedia:x:985: twitchdash , twitchbot ' "$test_passwd"
if "$INSTALLER" --validate-media-group-for-test \
    'twitchmedia:x:985:twitchbot' "$test_passwd" >/dev/null 2>&1; then
  echo "Installer akzeptiert eine unvollständige Mediengruppe." >&2
  exit 1
fi
if "$INSTALLER" --validate-media-group-for-test \
    'twitchmedia:x:985:twitchdash,fremd,twitchbot' "$test_passwd" >/dev/null 2>&1; then
  echo "Installer akzeptiert ein unerwartetes Mediengruppen-Mitglied." >&2
  exit 1
fi
if "$INSTALLER" --validate-media-group-for-test \
    'twitchmedia:x:985:twitchdash,twitchbot,twitchdash' "$test_passwd" >/dev/null 2>&1; then
  echo "Installer akzeptiert ein dupliziertes Mediengruppen-Mitglied." >&2
  exit 1
fi
if "$INSTALLER" --validate-media-group-for-test \
    'twitchmedia:x:985:twitchdash,twitchbot' \
    $'twitchbot:x:995:984::/var/empty:/usr/sbin/nologin\ntwitchdash:x:991:983::/var/empty:/usr/sbin/nologin\nfremd:x:990:985::/var/empty:/usr/sbin/nologin' \
    >/dev/null 2>&1; then
  echo "Installer übersieht ein fremdes Konto mit primärer Medien-GID." >&2
  exit 1
fi
if "$INSTALLER" --validate-media-group-for-test \
    'twitchmedia:x:985:twitchdash,twitchbot' \
    $'twitchbot:x:995:984::/var/empty:/usr/sbin/nologin\ntwitchdash:x:995:983::/var/empty:/usr/sbin/nologin' \
    >/dev/null 2>&1; then
  echo "Installer akzeptiert identische Bot-/Dashboard-UIDs." >&2
  exit 1
fi

build_release="$TEST_DIR/build-release"
build_helfer="$build_release/rust/scripts/stage-yt-dlp-release.sh"
build_checksum="$build_release/ops/systemd/yt-dlp-linux-2026.08.19.sha256"
mkdir -p "$build_release/rust/scripts" "$build_release/ops/systemd"
cp "$SCRIPT_DIR/../../rust/scripts/stage-yt-dlp-release.sh" "$build_helfer"
cp "$SCRIPT_DIR/yt-dlp-linux-2026.08.19.sha256" "$build_checksum"
if build_ausgabe="$(bash "$build_helfer" 2>&1)"; then
  echo "yt-dlp-Build-Helfer akzeptiert einen fehlenden Quellpfad." >&2
  exit 1
fi
if [[ "$build_ausgabe" != *"Aufruf:"* ]]; then
  echo "yt-dlp-Build-Helfer erklärt den Pflichtparameter nicht: $build_ausgabe" >&2
  exit 1
fi
if build_ausgabe="$(bash "$build_helfer" "$nicht_ausfuehrbar" 2>&1)"; then
  echo "yt-dlp-Build-Helfer akzeptiert eine nicht ausführbare Quelle." >&2
  exit 1
fi
if [[ "$build_ausgabe" != *"nicht ausführbar"* ]]; then
  echo "yt-dlp-Build-Helfer meldet eine nicht ausführbare Quelle unklar: $build_ausgabe" >&2
  exit 1
fi
if build_ausgabe="$(bash "$build_helfer" "$symlink" 2>&1)"; then
  echo "yt-dlp-Build-Helfer akzeptiert einen Symlink." >&2
  exit 1
fi
if [[ "$build_ausgabe" != *"darf kein Symlink sein"* ]]; then
  echo "yt-dlp-Build-Helfer meldet einen Symlink unklar: $build_ausgabe" >&2
  exit 1
fi

build_quellverzeichnis="$TEST_DIR/build-quellverzeichnis"
build_quelllink="$TEST_DIR/build-quelllink"
mkdir "$build_quellverzeichnis"
printf '#!/usr/bin/env bash\nprintf "2026.08.19\\n"\n' \
  >"$build_quellverzeichnis/yt-dlp_linux"
chmod 0755 "$build_quellverzeichnis/yt-dlp_linux"
ln -s "$build_quellverzeichnis" "$build_quelllink"
if build_ausgabe="$(bash "$build_helfer" "$build_quelllink/yt-dlp_linux" 2>&1)"; then
  echo "yt-dlp-Build-Helfer akzeptiert einen Symlink im Quellpfad." >&2
  exit 1
fi
if [[ "$build_ausgabe" != *"Quellpfad enthält einen Symlink"* ]]; then
  echo "yt-dlp-Build-Helfer meldet einen Symlink im Quellpfad unklar: $build_ausgabe" >&2
  exit 1
fi

build_quelle="$TEST_DIR/yt-dlp_linux"
printf '#!/usr/bin/env bash\nprintf "2026.08.19\\n"\n' >"$build_quelle"
chmod 0755 "$build_quelle"
if build_ausgabe="$(bash "$build_helfer" "$build_quelle" 2>&1)"; then
  echo "yt-dlp-Build-Helfer akzeptiert eine falsche Prüfsumme." >&2
  exit 1
fi
if [[ "$build_ausgabe" != *"yt-dlp-Prüfsumme stimmt nicht"* ]]; then
  echo "yt-dlp-Build-Helfer meldet die falsche Prüfsumme unklar: $build_ausgabe" >&2
  exit 1
fi
build_sha="$(sha256sum -- "$build_quelle")"
printf '%s  yt-dlp_linux\n' "${build_sha%% *}" >"$build_checksum"
bash "$build_helfer" "$build_quelle"
build_ziel="$build_release/rust/target/release/yt-dlp"
if [[ ! -f "$build_ziel" || -L "$build_ziel" || ! -x "$build_ziel" ]]; then
  echo "yt-dlp-Build-Helfer erzeugt kein reguläres ausführbares Ziel." >&2
  exit 1
fi
if ! cmp --silent -- "$build_quelle" "$build_ziel"; then
  echo "yt-dlp-Build-Helfer verändert den Artefaktinhalt." >&2
  exit 1
fi

falsche_version="$TEST_DIR/falsche-version/yt-dlp_linux"
mkdir -p "$(dirname "$falsche_version")"
printf '#!/usr/bin/env bash\nprintf "2026.07.04\\n"\n' >"$falsche_version"
chmod 0755 "$falsche_version"
falsche_version_sha="$(sha256sum -- "$falsche_version")"
printf '%s  yt-dlp_linux\n' "${falsche_version_sha%% *}" >"$build_checksum"
if build_ausgabe="$(bash "$build_helfer" "$falsche_version" 2>&1)"; then
  echo "yt-dlp-Build-Helfer akzeptiert die falsche Version." >&2
  exit 1
fi
if [[ "$build_ausgabe" != *"yt-dlp-Version stimmt nicht"* ]]; then
  echo "yt-dlp-Build-Helfer meldet die falsche Version unklar: $build_ausgabe" >&2
  exit 1
fi
printf '%s  yt-dlp_linux\n' "${build_sha%% *}" >"$build_checksum"

build_symlink_release="$TEST_DIR/build-symlink-release"
build_symlink_helfer="$build_symlink_release/rust/scripts/stage-yt-dlp-release.sh"
build_symlink_checksum="$build_symlink_release/ops/systemd/yt-dlp-linux-2026.08.19.sha256"
externes_ziel="$TEST_DIR/externes-build-ziel"
mkdir -p "$build_symlink_release/rust/scripts" "$build_symlink_release/ops/systemd" "$externes_ziel"
cp "$SCRIPT_DIR/../../rust/scripts/stage-yt-dlp-release.sh" "$build_symlink_helfer"
cp "$build_checksum" "$build_symlink_checksum"
ln -s "$externes_ziel" "$build_symlink_release/rust/target"
if build_ausgabe="$(bash "$build_symlink_helfer" "$build_quelle" 2>&1)"; then
  echo "yt-dlp-Build-Helfer akzeptiert ein Ziel über einen Symlink." >&2
  exit 1
fi
if [[ "$build_ausgabe" != *"Build-Zielpfad enthält einen Symlink"* ]]; then
  echo "yt-dlp-Build-Helfer meldet den Symlink-Zielpfad unklar: $build_ausgabe" >&2
  exit 1
fi
if [[ -e "$externes_ziel/release/yt-dlp" ]]; then
  echo "yt-dlp-Build-Helfer hat außerhalb des Buildbaums geschrieben." >&2
  exit 1
fi

wrapper_release="$TEST_DIR/wrapper-release"
wrapper="$wrapper_release/rust/scripts/run_tb_bot_service.sh"
mkdir -p "$wrapper_release/rust/scripts" "$wrapper_release/rust/target/release"
cp "$SCRIPT_DIR/../../rust/scripts/run_tb_bot_service.sh" "$wrapper"
wrapper_ausgabe="$(bash "$wrapper" 2>&1 || true)"
if [[ "$wrapper_ausgabe" != *"Gebündeltes yt-dlp fehlt oder ist nicht ausführbar"* ]]; then
  echo "Bot-Wrapper scheitert ohne yt-dlp nicht verständlich: $wrapper_ausgabe" >&2
  exit 1
fi

printf '#!/usr/bin/env bash\nexit 0\n' >"$wrapper_release/rust/target/release/yt-dlp"
chmod 0644 "$wrapper_release/rust/target/release/yt-dlp"
wrapper_ausgabe="$(bash "$wrapper" 2>&1 || true)"
if [[ "$wrapper_ausgabe" != *"Gebündeltes yt-dlp fehlt oder ist nicht ausführbar"* ]]; then
  echo "Bot-Wrapper akzeptiert nicht ausführbares yt-dlp: $wrapper_ausgabe" >&2
  exit 1
fi

echo "yt-dlp-Release-Artefaktprüfung: OK"
