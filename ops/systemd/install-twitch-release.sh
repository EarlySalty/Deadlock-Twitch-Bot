#!/usr/bin/env -S -i PATH=/usr/sbin:/usr/bin:/sbin:/bin LC_ALL=C /usr/bin/bash --noprofile --norc
# shellcheck shell=bash
set -euo pipefail
PATH=/usr/sbin:/usr/bin:/sbin:/bin
LC_ALL=C
builtin export PATH LC_ALL

# Der Installer läuft als root. Vererbte Tool-Optionen, Git-Umlenkungen und
# exportierte Shell-Funktionen dürfen deshalb keinen späteren Unterprozess
# beeinflussen. Bereits die Shebang startet Bash mit leerer Allowlist-Umgebung;
# damit werden BASH_ENV, Loader-Variablen und exportierte Funktionen vor dem
# ersten Bash-Befehl entfernt.
for unsafe_variable in \
  BASH_ENV CDPATH ENV GLOBIGNORE LD_AUDIT LD_LIBRARY_PATH LD_PRELOAD \
  POSIXLY_CORRECT TAR_OPTIONS TMPDIR; do
  if [[ -v "$unsafe_variable" ]]; then
    echo "Root-Installer wurde nicht mit einer vor Bash bereinigten Umgebung gestartet." >&2
    exit 1
  fi
done
mapfile -t inherited_functions < <(builtin compgen -A function)
if [[ ${#inherited_functions[@]} -ne 0 ]]; then
  echo "Root-Installer wurde mit einer geerbten Shell-Funktion gestartet." >&2
  exit 1
fi
builtin unset BASH_ENV CDPATH ENV GLOBIGNORE LD_AUDIT LD_LIBRARY_PATH LD_PRELOAD \
  POSIXLY_CORRECT TAR_OPTIONS TMPDIR
for git_variable in "${!GIT_@}"; do
  builtin unset "$git_variable"
done
builtin unalias -a

YT_DLP_VERSION='2026.08.19'
YT_DLP_RELEASE_PATH='rust/target/release/yt-dlp'
YT_DLP_CHECKSUM_PATH="ops/systemd/yt-dlp-linux-$YT_DLP_VERSION.sha256"
DOWNLOADER_RELEASE_PATH='rust/target/release/tb-media-downloader'
DOWNLOADER_SOCKET_PATH='ops/systemd/deadlock-twitch-media-downloader.socket'
DOWNLOADER_SERVICE_PATH='ops/systemd/deadlock-twitch-media-downloader@.service'
DOWNLOADER_FIREWALL_PATH='ops/systemd/deadlock-twitch-media-downloader-firewall.service'
DOWNLOADER_FIREWALL_REFRESH_PATH='ops/systemd/deadlock-twitch-media-downloader-firewall-refresh@.service'
DOWNLOADER_FIREWALL_VALIDATOR_PATH='ops/systemd/validate-tb-media-downloader-firewall.sh'
DOWNLOADER_RESOLVER_PATH='ops/systemd/tb-media-downloader-resolv.conf'
DOWNLOADER_NFT_PATH='ops/systemd/tb-media-downloader-egress.nft'
BWRAP_PATH='/usr/bin/bwrap'
BWRAP_MIN_VERSION='0.9.0'
NFT_PATH='/usr/sbin/nft'
NFT_MIN_VERSION='1.0.9'
NFT_PACKAGE_VERSION='1.0.9-1ubuntu0.1'

parse_yt_dlp_checksum_line() {
  local line="$1"
  if [[ ! "$line" =~ ^([0-9a-f]{64})[[:space:]][[:space:]]yt-dlp_linux$ ]]; then
    echo "Gepinntes yt-dlp-Prüfsummenmanifest ist ungültig." >&2
    return 1
  fi
  printf '%s\n' "${BASH_REMATCH[1]}"
}

read_yt_dlp_checksum_file() {
  local manifest="$1"
  local -a lines
  if [[ -L "$manifest" || ! -f "$manifest" ]]; then
    echo "Gepinntes yt-dlp-Prüfsummenmanifest fehlt oder ist ein Symlink: $manifest" >&2
    return 1
  fi
  mapfile -t lines <"$manifest"
  if [[ ${#lines[@]} -ne 1 ]]; then
    echo "Gepinntes yt-dlp-Prüfsummenmanifest ist ungültig: $manifest" >&2
    return 1
  fi
  parse_yt_dlp_checksum_line "${lines[0]}"
}

validate_no_symlink_components() {
  local artifact="$1"
  local resolved
  resolved="$(realpath -e -- "$artifact")" || {
    echo "Release-Artefaktpfad kann nicht aufgelöst werden: $artifact" >&2
    return 1
  }
  if [[ "$resolved" != "$artifact" ]]; then
    echo "Release-Artefaktpfad enthält einen Symlink: $artifact" >&2
    return 1
  fi
}

validate_yt_dlp_artifact() {
  local artifact="$1"
  local expected_checksum="$2"
  local actual_checksum
  if [[ -L "$artifact" ]]; then
    echo "yt-dlp-Release-Artefakt darf kein Symlink sein: $artifact" >&2
    return 1
  fi
  if [[ ! -e "$artifact" ]]; then
    echo "yt-dlp-Release-Artefakt fehlt: $artifact" >&2
    return 1
  fi
  validate_no_symlink_components "$artifact"
  if [[ ! -f "$artifact" ]]; then
    echo "yt-dlp-Release-Artefakt ist keine reguläre Datei: $artifact" >&2
    return 1
  fi
  if [[ ! -x "$artifact" ]]; then
    echo "yt-dlp-Release-Artefakt ist nicht ausführbar: $artifact" >&2
    return 1
  fi
  actual_checksum="$(sha256sum -- "$artifact")"
  actual_checksum="${actual_checksum%% *}"
  if [[ "$actual_checksum" != "$expected_checksum" ]]; then
    echo "yt-dlp-Prüfsumme stimmt nicht: $artifact" >&2
    return 1
  fi
}

validate_public_resolver_file() {
  local resolver_file="$1"
  local expected_uid="$2"
  local expected_gid="$3"
  local -a resolver_lines
  if [[ -L "$resolver_file" || ! -f "$resolver_file" ]]; then
    echo "Versionierte öffentliche Resolverdatei fehlt oder ist ein Symlink: $resolver_file" >&2
    return 1
  fi
  validate_no_symlink_components "$resolver_file"
  if [[ "$(stat -c '%u:%g:%h:%a' -- "$resolver_file")" != \
        "$expected_uid:$expected_gid:1:644" ]]; then
    echo "Versionierte öffentliche Resolverdatei hat unsichere Eigentümer-, Link- oder Modusdaten: $resolver_file" >&2
    return 1
  fi
  mapfile -t resolver_lines <"$resolver_file"
  if [[ ${#resolver_lines[@]} -ne 5 ||
        "${resolver_lines[0]}" != 'nameserver 1.1.1.1' ||
        "${resolver_lines[1]}" != 'nameserver 1.0.0.1' ||
        "${resolver_lines[2]}" != 'nameserver 2606:4700:4700::1111' ||
        "${resolver_lines[3]}" != 'nameserver 2606:4700:4700::1001' ||
        "${resolver_lines[4]}" != 'options timeout:2 attempts:2 rotate edns0' ]]; then
    echo "Versionierte öffentliche Resolverdatei weicht von der geprüften Allowlist ab: $resolver_file" >&2
    return 1
  fi
}

validate_downloader_nft_ruleset() {
  local ruleset_file="$1"
  local expected_uid="$2"
  local expected_gid="$3"
  if [[ -L "$ruleset_file" || ! -f "$ruleset_file" ]]; then
    echo "Versionierter Downloader-nft-Regelsatz fehlt oder ist ein Symlink: $ruleset_file" >&2
    return 1
  fi
  validate_no_symlink_components "$ruleset_file"
  if [[ "$(stat -c '%u:%g:%h:%a' -- "$ruleset_file")" != \
        "$expected_uid:$expected_gid:1:644" ]]; then
    echo "Downloader-nft-Regelsatz hat unsichere Eigentümer-, Link- oder Modusdaten: $ruleset_file" >&2
    return 1
  fi
  if ! "$NFT_PATH" --check --file "$ruleset_file"; then
    echo "Downloader-nft-Regelsatz ist auf diesem Host nicht atomar ladbar." >&2
    return 1
  fi
}

version_at_least() {
  local actual="$1"
  local minimum="$2"
  local index actual_part minimum_part
  local -a actual_parts minimum_parts
  if [[ ! "$actual" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ||
        ! "$minimum" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    return 1
  fi
  IFS='.' read -r -a actual_parts <<<"$actual"
  IFS='.' read -r -a minimum_parts <<<"$minimum"
  for index in 0 1 2; do
    actual_part=$((10#${actual_parts[index]}))
    minimum_part=$((10#${minimum_parts[index]}))
    if ((actual_part > minimum_part)); then
      return 0
    fi
    if ((actual_part < minimum_part)); then
      return 1
    fi
  done
  return 0
}

validate_bwrap_binary() {
  local artifact="$1"
  local minimum_version="$2"
  local expected_uid="$3"
  local expected_gid="$4"
  local version_output actual_version capabilities help_output required_option
  if [[ -L "$artifact" || ! -e "$artifact" ]]; then
    echo "Bubblewrap fehlt oder ist ein Symlink: $artifact" >&2
    return 1
  fi
  validate_no_symlink_components "$artifact"
  if [[ ! -f "$artifact" || ! -x "$artifact" ]]; then
    echo "Bubblewrap ist keine reguläre ausführbare Datei: $artifact" >&2
    return 1
  fi
  if [[ "$(stat -c '%u:%g:%h' -- "$artifact")" != "$expected_uid:$expected_gid:1" ]] ||
     [[ -n "$(find "$artifact" -maxdepth 0 -perm /022 -print -quit)" ]] ||
     [[ -u "$artifact" || -g "$artifact" ]]; then
    echo "Bubblewrap hat unsichere Eigentümer-, Link- oder Modusdaten: $artifact" >&2
    return 1
  fi
  capabilities="$(/usr/sbin/getcap -- "$artifact")"
  if [[ -n "$capabilities" ]]; then
    echo "Bubblewrap darf keine Dateisystem-Capabilities tragen: $artifact" >&2
    return 1
  fi
  version_output="$("$artifact" --version)" || {
    echo "Bubblewrap-Version kann nicht gelesen werden: $artifact" >&2
    return 1
  }
  if [[ ! "$version_output" =~ ^bubblewrap[[:space:]]([0-9]+\.[0-9]+\.[0-9]+)$ ]]; then
    echo "Bubblewrap meldet ein unerwartetes Versionsformat: $version_output" >&2
    return 1
  fi
  actual_version="${BASH_REMATCH[1]}"
  if ! version_at_least "$actual_version" "$minimum_version"; then
    echo "Bubblewrap ist zu alt: benötigt wird mindestens $minimum_version, gefunden wurde $actual_version." >&2
    return 1
  fi
  help_output="$("$artifact" --help)" || {
    echo "Bubblewrap-Funktionsumfang kann nicht geprüft werden: $artifact" >&2
    return 1
  }
  for required_option in \
    --ro-bind-fd --bind-fd --file --unshare-user --unshare-pid --unshare-net \
    --disable-userns --cap-drop --clearenv --die-with-parent; do
    if [[ "$help_output" != *"$required_option"* ]]; then
      echo "Bubblewrap unterstützt die benötigte Option nicht: $required_option" >&2
      return 1
    fi
  done
}

validate_nft_binary() {
  local artifact="$1"
  local minimum_version="$2"
  local expected_uid="$3"
  local expected_gid="$4"
  local version_output actual_version capabilities help_output describe_output
  if [[ -L "$artifact" || ! -e "$artifact" ]]; then
    echo "nftables-Werkzeug fehlt oder ist ein Symlink: $artifact" >&2
    return 1
  fi
  validate_no_symlink_components "$artifact"
  if [[ ! -f "$artifact" || ! -x "$artifact" ]]; then
    echo "nftables-Werkzeug ist keine reguläre ausführbare Datei: $artifact" >&2
    return 1
  fi
  if [[ "$(stat -c '%u:%g:%h' -- "$artifact")" != "$expected_uid:$expected_gid:1" ]] ||
     [[ -n "$(find "$artifact" -maxdepth 0 -perm /022 -print -quit)" ]] ||
     [[ -u "$artifact" || -g "$artifact" ]]; then
    echo "nftables-Werkzeug hat unsichere Eigentümer-, Link- oder Modusdaten: $artifact" >&2
    return 1
  fi
  capabilities="$(/usr/sbin/getcap -- "$artifact")"
  if [[ -n "$capabilities" ]]; then
    echo "nftables-Werkzeug darf keine Dateisystem-Capabilities tragen: $artifact" >&2
    return 1
  fi
  version_output="$("$artifact" --version)" || {
    echo "nftables-Version kann nicht gelesen werden: $artifact" >&2
    return 1
  }
  if [[ ! "$version_output" =~ ^nftables[[:space:]]v([0-9]+\.[0-9]+\.[0-9]+)([[:space:]].*)?$ ]]; then
    echo "nftables meldet ein unerwartetes Versionsformat: $version_output" >&2
    return 1
  fi
  actual_version="${BASH_REMATCH[1]}"
  if ! version_at_least "$actual_version" "$minimum_version"; then
    echo "nftables ist zu alt: benötigt wird mindestens $minimum_version, gefunden wurde $actual_version." >&2
    return 1
  fi
  help_output="$("$artifact" --help)" || {
    echo "nftables-Funktionsumfang kann nicht geprüft werden: $artifact" >&2
    return 1
  }
  if [[ "$help_output" != *'--check'* || "$help_output" != *'--file'* ]]; then
    echo "nftables unterstützt die benötigte atomare Datei-/Check-Schnittstelle nicht." >&2
    return 1
  fi
  describe_output="$("$artifact" describe meta skuid)" || {
    echo "nftables unterstützt die benötigte Socket-UID-Auswahl nicht." >&2
    return 1
  }
  if [[ "$describe_output" != *'skuid'* ]]; then
    echo "nftables bestätigt meta skuid nicht." >&2
    return 1
  fi
}

validate_nft_package_state() {
  local package_state generic_enabled generic_active ldd_output package_name verify_output
  local -a nft_packages=(
    nftables:amd64 libnftables1:amd64 libnftnl11:amd64 libmnl0:amd64
    libedit2:amd64 libc6:amd64 iptables:amd64 libjansson4:amd64
    libgmp10:amd64 libtinfo6:amd64 libbsd0:amd64 libmd0:amd64
  )
  if [[ "$(/usr/bin/dpkg --print-architecture)" != amd64 ]]; then
    echo "Der gepinnte nftables-/yt-dlp-Releasevertrag erwartet einen amd64-Host." >&2
    return 1
  fi
  package_state="$(/usr/bin/dpkg-query -W \
    -f='${db:Status-Abbrev} ${Version}' nftables:amd64 2>/dev/null || true)"
  if [[ "$package_state" != "ii  $NFT_PACKAGE_VERSION" ]]; then
    echo "Erwartet wird das geprüfte Ubuntu-Paket nftables $NFT_PACKAGE_VERSION." >&2
    return 1
  fi
  for package_name in "${nft_packages[@]}"; do
    verify_output="$(/usr/bin/dpkg --verify "$package_name" 2>&1)" || {
      echo "dpkg kann nftables oder eine geladene Bibliotheksabhängigkeit nicht verifizieren: $package_name" >&2
      return 1
    }
    if [[ -n "$verify_output" ]]; then
      echo "nftables-Paketdateien weichen vom dpkg-Stand ab: $package_name" >&2
      return 1
    fi
  done
  ldd_output="$(/usr/bin/env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin LC_ALL=C \
    /usr/bin/ldd "$NFT_PATH")" || {
    echo "nftables-Bibliotheksabhängigkeiten können nicht aufgelöst werden." >&2
    return 1
  }
  if [[ "$ldd_output" == *'not found'* ]]; then
    echo "nftables hat eine nicht aufgelöste Bibliotheksabhängigkeit." >&2
    return 1
  fi
  generic_enabled="$(/usr/bin/systemctl is-enabled nftables.service 2>/dev/null || true)"
  generic_active="$(/usr/bin/systemctl is-active nftables.service 2>/dev/null || true)"
  if [[ "$generic_enabled" != disabled || "$generic_active" != inactive ]]; then
    echo "Generischer nftables.service muss disabled und inactive bleiben; er wird nicht automatisch gestoppt." >&2
    return 1
  fi
}

probe_bwrap_fd_sandbox() {
  local account="$1"
  local account_uid account_gid probe_dir probe_input probe_foreign probe_output probe_result probe_resolver
  local probe_ok=0
  account_uid="$(/usr/bin/id -u "$account")"
  account_gid="$(/usr/bin/id -g "$account")"
  probe_dir="$(/usr/bin/mktemp -d "/run/tb-bwrap-probe-${account}.XXXXXXXX")"
  probe_input="$probe_dir/input"
  probe_foreign="$probe_dir/foreign"
  probe_output="$probe_dir/output"
  probe_result="$probe_output/result"
  probe_resolver="$probe_dir/resolv.conf"
  /usr/bin/install -d -o "$account_uid" -g "$account_gid" -m 0700 \
    "$probe_dir" "$probe_output"
  printf 'bubblewrap-fd-probe\n' >"$probe_input"
  printf 'darf-nicht-in-die-sandbox\n' >"$probe_foreign"
  printf 'nameserver 1.1.1.1\n' >"$probe_resolver"
  /usr/bin/chown --no-dereference "$account_uid:$account_gid" -- \
    "$probe_input" "$probe_foreign" "$probe_resolver"
  /usr/bin/chmod 0640 -- "$probe_input" "$probe_foreign" "$probe_resolver"

  # Der String wird absichtlich erst von der unprivilegierten Kind-Bash
  # ausgewertet; $1/$2/$3 sind deren feste Positionsparameter.
  # shellcheck disable=SC2016
  if /usr/bin/setpriv \
      --reuid="$account_uid" --regid="$account_gid" --clear-groups \
      --no-new-privs --bounding-set=-all --inh-caps=-all --ambient-caps=-all \
      /usr/bin/env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin LC_ALL=C \
      /usr/bin/bash --noprofile --norc -c '
        set -euo pipefail
        # Bubblewrap 0.9.0 schließt fremde non-CLOEXEC-FDs nicht selbst. Diese
        # Probe bildet deshalb die Runtime-Allowlist nach: ein bewusst offen
        # geerbter FD 7 wird geschlossen, nur Input 8 und Output 9 bleiben.
        exec 7<"$4"
        exec 8<"$2"
        exec 9<"$3"
        exec 10<"$5"
        for fd_path in /proc/self/fd/*; do
          fd="${fd_path##*/}"
          if [[ "$fd" =~ ^[0-9]+$ && "$fd" -gt 2 &&
                "$fd" -ne 8 && "$fd" -ne 9 && "$fd" -ne 10 ]]; then
            eval "exec ${fd}<&-"
          fi
        done
        exec "$1" \
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
          --setenv LC_ALL C -- /usr/bin/bash --noprofile --norc -c '\''
            if { : <&7; } 2>/dev/null; then
              exit 97
            fi
            IFS= read -r resolver_value </etc/resolv.conf
            [[ "$resolver_value" == "nameserver 1.1.1.1" ]]
            IFS= read -r probe_value </input
            printf "%s\\n" "$probe_value" >/output/result
          '\''
      ' bwrap-probe "$BWRAP_PATH" "$probe_input" "$probe_output" "$probe_foreign" "$probe_resolver" \
      >/dev/null 2>&1 &&
     [[ -f "$probe_result" && ! -L "$probe_result" ]] &&
     /usr/bin/cmp --silent -- "$probe_input" "$probe_result"; then
    probe_ok=1
  fi
  find "$probe_dir" -xdev -depth -delete
  if [[ "$probe_ok" -ne 1 ]]; then
    echo "Bubblewrap-FD-Sandboxprobe ist für $account fehlgeschlagen." >&2
    return 1
  fi
}

validate_media_tree_contents() {
  local clips_root="$1"
  local rendered_dir="$clips_root/rendered"
  local uploads_dir="$clips_root/uploads"
  local dashboard_work_dir="$uploads_dir/.dashboard-work"
  local preparation_work_dir="$clips_root/.preparation-work"
  local retention_dir="$clips_root/.retention-quarantine"
  local unsafe_entry unsafe_directory mount_target directory basename work_entry
  local upload_entry upload_relative

  unsafe_entry="$(find "$clips_root" -xdev ! \( -type f -o -type d \) -print -quit)"
  if [[ -n "$unsafe_entry" ]]; then
    echo "Medienbaum enthält einen Symlink oder Sonderdateityp: $unsafe_entry" >&2
    return 1
  fi
  # Root-Quelldateien sind direkt unter clips erlaubt. Unterverzeichnisse
  # gehören ausschließlich zu den zwei getrennten Writer-Bereichen. Die drei
  # versteckten Arbeitsordner sind enge interne Verträge; weitere Dot-
  # Verzeichnisse oder Unterverzeichnisse darin sind nicht erlaubt.
  unsafe_directory=
  while IFS= read -r -d '' directory; do
    basename="${directory##*/}"
    if [[ "$directory" == "$dashboard_work_dir" ||
          "$directory" == "$preparation_work_dir" ||
          "$directory" == "$retention_dir" ]]; then
      continue
    fi
    if [[ "${directory#"$dashboard_work_dir"/}" != "$directory" ||
          "${directory#"$preparation_work_dir"/}" != "$directory" ||
          "${directory#"$retention_dir"/}" != "$directory" ||
          "$basename" == .* ]]; then
      unsafe_directory="$directory"
      break
    fi
    if [[ "$directory" == "$rendered_dir" || "$directory" == "$uploads_dir" ]] ||
       [[ "${directory#"$rendered_dir"/}" != "$directory" ]]; then
      continue
    fi
    upload_relative="${directory#"$uploads_dir"/}"
    if [[ "$upload_relative" != "$directory" && "$upload_relative" != */* &&
          "$upload_relative" =~ ^[a-z0-9_-]{1,128}$ ]]; then
      continue
    fi
    unsafe_directory="$directory"
    break
  done < <(find "$clips_root" -xdev -mindepth 1 -type d -print0)
  if [[ -n "$unsafe_directory" ]]; then
    echo "Medienbaum enthält ein unerwartetes Unterverzeichnis: $unsafe_directory" >&2
    return 1
  fi

  if [[ -d "$dashboard_work_dir" ]]; then
    while IFS= read -r -d '' work_entry; do
      basename="${work_entry##*/}"
      if [[ ! "$basename" =~ ^\.upload-[0-9a-f]{32}\.tmp\.mp4$ ]]; then
        echo "Dashboard-Arbeitsordner enthält eine unerwartete Datei: $work_entry" >&2
        return 1
      fi
    done < <(find "$dashboard_work_dir" -xdev -mindepth 1 -maxdepth 1 -type f -print0)
  fi

  if [[ -d "$uploads_dir" ]]; then
    while IFS= read -r -d '' upload_entry; do
      upload_relative="${upload_entry#"$uploads_dir"/}"
      if [[ ! "$upload_relative" =~ ^[a-z0-9_-]{1,128}/manual:[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\.mp4$ ]]; then
        echo "Upload-Baum enthält keine servergenerierte Manual-Upload-Datei: $upload_entry" >&2
        return 1
      fi
    done < <(find "$uploads_dir" -xdev -type f \
      ! -path "$dashboard_work_dir/*" -print0)
  fi

  if [[ -d "$preparation_work_dir" ]]; then
    while IFS= read -r -d '' work_entry; do
      basename="${work_entry##*/}"
      if [[ ! "$basename" =~ ^[1-9][0-9]*-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}-source\.tmp\.mp4$ &&
            ! "$basename" =~ ^[1-9][0-9]*-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}-[0-9a-f]{64}-render\.tmp\.mp4$ ]]; then
        echo "Preparation-Arbeitsordner enthält eine unerwartete Datei: $work_entry" >&2
        return 1
      fi
    done < <(find "$preparation_work_dir" -xdev -mindepth 1 -maxdepth 1 -type f -print0)
  fi

  if [[ -d "$retention_dir" ]]; then
    while IFS= read -r -d '' work_entry; do
      basename="${work_entry##*/}"
      if [[ ! "$basename" =~ ^[1-9][0-9]*-source-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\.mp4$ &&
            ! "$basename" =~ ^[1-9][0-9]*-render-[0-9a-f]{64}-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\.mp4$ ]]; then
        echo "Retention-Quarantäne enthält eine unerwartete Datei: $work_entry" >&2
        return 1
      fi
    done < <(find "$retention_dir" -xdev -mindepth 1 -maxdepth 1 -type f -print0)
  fi

  validate_media_hardlinks "$clips_root"

  # `find -xdev` darf keine eingehängte Teilstruktur überspringen. Der Root
  # selbst darf ein eigener Mount sein, darunter ist jeder weitere Mount ein
  # harter Fehler.
  while IFS= read -r mount_target; do
    if [[ "$mount_target" != "$clips_root" ]] &&
       [[ "${mount_target#"$clips_root"/}" != "$mount_target" ]]; then
      echo "Medienbaum enthält einen fremden Mount: $mount_target" >&2
      return 1
    fi
  done < <(/usr/bin/findmnt -rnR -o TARGET -T "$clips_root")
}

validate_media_hardlinks() {
  local clips_root="$1"
  local uploads_dir="$clips_root/uploads"
  local work_dir="$uploads_dir/.dashboard-work"
  local linked_file same_file key link_count relative
  local internal_count work_count final_count invalid_path
  declare -A checked_inodes=()

  while IFS= read -r -d '' linked_file; do
    key="$(stat -Lc '%d:%i' -- "$linked_file")" || return 1
    if [[ -v "checked_inodes[$key]" ]]; then
      continue
    fi
    checked_inodes[$key]=1
    link_count="$(stat -Lc '%h' -- "$linked_file")" || return 1
    internal_count=0
    work_count=0
    final_count=0
    invalid_path=
    while IFS= read -r -d '' same_file; do
      ((internal_count += 1))
      relative="${same_file#"$work_dir"/}"
      if [[ "$relative" != "$same_file" && "$relative" != */* &&
            "$relative" =~ ^\.upload-[0-9a-f]{32}\.tmp\.mp4$ ]]; then
        ((work_count += 1))
        continue
      fi
      relative="${same_file#"$uploads_dir"/}"
      if [[ "$relative" != "$same_file" &&
            "$relative" =~ ^[a-z0-9_-]{1,128}/manual:[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\.mp4$ ]]; then
        ((final_count += 1))
        continue
      fi
      invalid_path="$same_file"
    done < <(find "$clips_root" -xdev -type f -samefile "$linked_file" -print0)

    if [[ "$link_count" -ne 2 || "$internal_count" -ne "$link_count" ||
          "$work_count" -ne 1 || "$final_count" -ne 1 || -n "$invalid_path" ]]; then
      echo "Medienbaum enthält eine unerlaubt mehrfach verlinkte Datei: $linked_file" >&2
      return 1
    fi
  done < <(find "$clips_root" -xdev -type f -links +1 -print0)
}

harden_media_tree() {
  local media_parent="$1"
  local bot_owner="$2"
  local dashboard_owner="$3"
  local media_group="$4"
  local parent_owner="$5"
  local clips_root="$media_parent/clips"
  local rendered_dir="$clips_root/rendered"
  local uploads_dir="$clips_root/uploads"
  local dashboard_work_dir="$uploads_dir/.dashboard-work"
  local preparation_work_dir="$clips_root/.preparation-work"
  local retention_dir="$clips_root/.retention-quarantine"
  local bot_uid dashboard_uid retained_file retained_basename retained_uid

  if [[ -L "$media_parent" || ! -d "$media_parent" ]] ||
     [[ "$(realpath -e -- "$media_parent")" != "$media_parent" ]]; then
    echo "Medienwurzel fehlt oder enthält einen Symlink: $media_parent" >&2
    return 1
  fi

  # Zuerst den Parent gegen Rename-/Austausch-Races schließen. Während der
  # anschließenden Prüfung kann kein Laufzeitnutzer den clips-Eintrag ersetzen.
  /usr/bin/chown --no-dereference "$parent_owner:$media_group" -- "$media_parent"
  /usr/bin/chmod 0750 -- "$media_parent"
  if [[ ! -e "$clips_root" ]]; then
    /usr/bin/install -d -o "$parent_owner" -g "$media_group" -m 0700 "$clips_root"
  fi
  if [[ -L "$clips_root" || ! -d "$clips_root" ]] ||
     [[ "$(realpath -e -- "$clips_root")" != "$clips_root" ]]; then
    echo "Clips-Wurzel fehlt oder enthält einen Symlink: $clips_root" >&2
    return 1
  fi
  /usr/bin/chown --no-dereference "$parent_owner:$media_group" -- "$clips_root"
  /usr/bin/chmod 0700 -- "$clips_root"

  validate_media_tree_contents "$clips_root"
  /usr/bin/install -d -o "$bot_owner" -g "$media_group" -m 2750 "$rendered_dir"
  /usr/bin/install -d -o "$dashboard_owner" -g "$media_group" -m 2770 "$uploads_dir"
  /usr/bin/install -d -o "$dashboard_owner" -g "$media_group" -m 0700 "$dashboard_work_dir"
  /usr/bin/install -d -o "$bot_owner" -g "$media_group" -m 0700 "$preparation_work_dir"
  /usr/bin/install -d -o "$bot_owner" -g "$media_group" -m 0700 "$retention_dir"

  find "$clips_root" -xdev -mindepth 1 -maxdepth 1 -type f \
    -exec /usr/bin/chown --no-dereference "$bot_owner:$media_group" -- {} + \
    -exec /usr/bin/chmod 0640 -- {} +
  find "$rendered_dir" -xdev -type d \
    -exec /usr/bin/chown --no-dereference "$bot_owner:$media_group" -- {} + \
    -exec /usr/bin/chmod 2750 -- {} +
  find "$rendered_dir" -xdev -type f \
    -exec /usr/bin/chown --no-dereference "$bot_owner:$media_group" -- {} + \
    -exec /usr/bin/chmod 0640 -- {} +
  find "$uploads_dir" -xdev -type d ! -path "$dashboard_work_dir" \
    -exec /usr/bin/chown --no-dereference "$dashboard_owner:$media_group" -- {} + \
    -exec /usr/bin/chmod 2770 -- {} +
  find "$uploads_dir" -xdev -type f \
    -exec /usr/bin/chown --no-dereference "$dashboard_owner:$media_group" -- {} + \
    -exec /usr/bin/chmod 0640 -- {} +
  /usr/bin/chown --no-dereference "$dashboard_owner:$media_group" -- "$dashboard_work_dir"
  /usr/bin/chmod 0700 -- "$dashboard_work_dir"

  find "$preparation_work_dir" -xdev -mindepth 1 -maxdepth 1 -type f \
    -exec /usr/bin/chown --no-dereference "$bot_owner:$media_group" -- {} + \
    -exec /usr/bin/chmod 0640 -- {} +
  /usr/bin/chown --no-dereference "$bot_owner:$media_group" -- "$preparation_work_dir"
  /usr/bin/chmod 0700 -- "$preparation_work_dir"

  if [[ "$bot_owner" =~ ^[0-9]+$ ]]; then
    bot_uid="$bot_owner"
  else
    bot_uid="$(/usr/bin/id -u "$bot_owner")"
  fi
  if [[ "$dashboard_owner" =~ ^[0-9]+$ ]]; then
    dashboard_uid="$dashboard_owner"
  else
    dashboard_uid="$(/usr/bin/id -u "$dashboard_owner")"
  fi
  while IFS= read -r -d '' retained_file; do
    retained_basename="${retained_file##*/}"
    if [[ "$retained_basename" == *-render-* ]]; then
      /usr/bin/chown --no-dereference "$bot_owner:$media_group" -- "$retained_file"
    else
      retained_uid="$(stat -c '%u' -- "$retained_file")"
      if [[ "$retained_uid" != "$bot_uid" && "$retained_uid" != "$dashboard_uid" ]]; then
        echo "Retention-Quelle hat einen unerwarteten Eigentümer: $retained_file" >&2
        return 1
      fi
      /usr/bin/chown --no-dereference "$retained_uid:$media_group" -- "$retained_file"
    fi
    /usr/bin/chmod 0640 -- "$retained_file"
  done < <(find "$retention_dir" -xdev -mindepth 1 -maxdepth 1 -type f -print0)
  /usr/bin/chown --no-dereference "$bot_owner:$media_group" -- "$retention_dir"
  /usr/bin/chmod 0700 -- "$retention_dir"

  /usr/bin/chown --no-dereference "$bot_owner:$media_group" -- "$clips_root"
  /usr/bin/chmod 2750 -- "$clips_root"
  validate_media_tree_contents "$clips_root"
}

validate_media_group_record() {
  local group_record="$1"
  local passwd_records="$2"
  local group_name group_password group_gid group_members member trimmed
  local username password uid primary_gid rest
  local bot_present=0
  local dashboard_present=0
  local bot_uid=
  local dashboard_uid=
  local -a configured_members

  IFS=':' read -r group_name group_password group_gid group_members <<<"$group_record"
  if [[ "$group_name" != twitchmedia || ! "$group_gid" =~ ^[0-9]+$ ]]; then
    echo "Mediengruppe twitchmedia hat keinen eindeutigen numerischen GID." >&2
    return 1
  fi
  : "$group_password"
  IFS=',' read -r -a configured_members <<<"$group_members"
  for member in "${configured_members[@]}"; do
    trimmed="${member#"${member%%[![:space:]]*}"}"
    trimmed="${trimmed%"${trimmed##*[![:space:]]}"}"
    case "$trimmed" in
      twitchbot) ((bot_present += 1)) ;;
      twitchdash) ((dashboard_present += 1)) ;;
      *)
        echo "Mediengruppe twitchmedia enthält ein unerwartetes Laufzeitkonto." >&2
        return 1
        ;;
    esac
  done
  if [[ $bot_present -ne 1 || $dashboard_present -ne 1 ]]; then
    echo "Mediengruppe twitchmedia fehlt oder hat nicht die erwarteten Laufzeitkonten." >&2
    return 1
  fi

  while IFS=':' read -r username password uid primary_gid rest; do
    : "$password" "$rest"
    case "$username" in
      twitchbot) bot_uid="$uid" ;;
      twitchdash) dashboard_uid="$uid" ;;
    esac
    if [[ "$primary_gid" == "$group_gid" ]]; then
      echo "Mediengruppe twitchmedia ist unerwartet primäre Gruppe eines Kontos." >&2
      return 1
    fi
  done <<<"$passwd_records"
  if [[ ! "$bot_uid" =~ ^[0-9]+$ || ! "$dashboard_uid" =~ ^[0-9]+$ ||
        "$bot_uid" == "$dashboard_uid" ]]; then
    echo "Laufzeitkonten für Medienrechte haben keine getrennten UIDs." >&2
    return 1
  fi
}

validate_downloader_account_records() {
  local passwd_record="$1"
  local group_record="$2"
  local all_passwd_records="$3"
  local membership_gids="$4"
  local username password uid primary_gid gecos home shell
  local group_name group_password group_gid group_members
  local account_record account_name account_password account_uid account_gid account_rest
  local passwd_colons group_colons account_colons
  local matching_record_count=0
  local -a parsed_membership_gids

  passwd_colons="${passwd_record//[^:]/}"
  if [[ "$passwd_record" == *$'\n'* ||
        "${#passwd_record}" -eq 0 ||
        "${#passwd_colons}" -ne 6 ]]; then
    echo "Downloader-Konto ist nicht eindeutig oder hat ein ungültiges passwd-Format." >&2
    return 1
  fi
  IFS=':' read -r username password uid primary_gid gecos home shell <<<"$passwd_record"
  : "$password" "$gecos"
  if [[ "$username" != twitchdownload ||
        ! "$uid" =~ ^[0-9]+$ || "$uid" == 0 ||
        ! "$primary_gid" =~ ^[0-9]+$ || "$primary_gid" == 0 ||
        "$home" != /nonexistent || "$shell" != /usr/sbin/nologin ]]; then
    echo "Downloader-Konto muss eine eigene UID/GID, /nonexistent und /usr/sbin/nologin verwenden." >&2
    return 1
  fi

  group_colons="${group_record//[^:]/}"
  if [[ "$group_record" == *$'\n'* ||
        "${#group_colons}" -ne 3 ]]; then
    echo "Downloader-Primärgruppe ist nicht eindeutig oder ungültig." >&2
    return 1
  fi
  IFS=':' read -r group_name group_password group_gid group_members <<<"$group_record"
  : "$group_password"
  if [[ "$group_name" != twitchdownload || "$group_gid" != "$primary_gid" ||
        -n "$group_members" ]]; then
    echo "Downloader-Primärgruppe muss eigenständig und ohne explizite Mitglieder sein." >&2
    return 1
  fi

  while IFS= read -r account_record; do
    [[ -n "$account_record" ]] || continue
    account_colons="${account_record//[^:]/}"
    if [[ "${#account_colons}" -ne 6 ]]; then
      echo "passwd-Daten für die Downloader-Isolation sind ungültig." >&2
      return 1
    fi
    IFS=':' read -r account_name account_password account_uid account_gid account_rest \
      <<<"$account_record"
    : "$account_name" "$account_password" "$account_rest"
    if [[ "$account_record" == "$passwd_record" ]]; then
      ((matching_record_count += 1))
      continue
    fi
    if [[ "$account_uid" == "$uid" || "$account_gid" == "$primary_gid" ]]; then
      echo "Downloader teilt UID oder Primär-GID mit einem anderen Konto." >&2
      return 1
    fi
  done <<<"$all_passwd_records"
  if [[ "$matching_record_count" -ne 1 ]]; then
    echo "Downloader-Konto ist in der NSS-passwd-Sicht nicht genau einmal vorhanden." >&2
    return 1
  fi

  read -r -a parsed_membership_gids <<<"$membership_gids"
  if [[ ${#parsed_membership_gids[@]} -ne 1 ||
        "${parsed_membership_gids[0]}" != "$primary_gid" ]]; then
    echo "Downloader-Konto darf keiner zusätzlichen Gruppe angehören." >&2
    return 1
  fi
}

validate_downloader_runtime_account() {
  local passwd_record group_record all_passwd_records membership_gids
  if ! passwd_record="$(/usr/bin/getent passwd twitchdownload)" ||
     ! group_record="$(/usr/bin/getent group twitchdownload)"; then
    echo "Isoliertes Laufzeitkonto twitchdownload samt eigener Gruppe fehlt." >&2
    return 1
  fi
  all_passwd_records="$(/usr/bin/getent passwd)"
  membership_gids="$(/usr/bin/id -G twitchdownload)"
  validate_downloader_account_records \
    "$passwd_record" "$group_record" "$all_passwd_records" "$membership_gids"
  if /usr/bin/pgrep -u twitchdownload >/dev/null; then
    echo "Downloader-Releasewechsel erfordert vollständig gestopptes twitchdownload." >&2
    return 1
  fi
  if /usr/bin/systemctl is-active --quiet deadlock-twitch-media-downloader.socket 2>/dev/null; then
    echo "Downloader-Releasewechsel erfordert einen vollständig gestoppten Aktivierungs-Socket." >&2
    return 1
  fi
}

validate_offline_unit_gates() {
  local unit state enable_link
  local -a offline_units=(
    deadlock-twitch-bot-rust.service
    deadlock-twitch-dashboard-rust.service
    deadlock-twitch-media-downloader.socket
  )
  for unit in "${offline_units[@]}"; do
    state="$(/usr/bin/systemctl is-enabled "$unit" 2>/dev/null || true)"
    if [[ "$state" != masked-runtime ]] ||
       [[ ! -L "/run/systemd/system/$unit" ]] ||
       [[ "$(/usr/bin/readlink -- "/run/systemd/system/$unit")" != /dev/null ]]; then
      echo "Offline-Cutover erfordert eine exakte Runtime-Maske: $unit" >&2
      return 1
    fi
    if /usr/bin/systemctl is-active --quiet "$unit" 2>/dev/null; then
      echo "Offline-Cutover erfordert eine gestoppte Unit: $unit" >&2
      return 1
    fi
    enable_link="$(/usr/bin/find /etc/systemd/system -xdev -type l \
      -name "$unit" -print -quit)"
    if [[ -n "$enable_link" ]]; then
      echo "Offline-Cutover erfordert eine persistent deaktivierte Unit: $unit" >&2
      return 1
    fi
  done
}

run_as_downloader() {
  local downloader_uid downloader_gid
  downloader_uid="$(/usr/bin/id -u twitchdownload)"
  downloader_gid="$(/usr/bin/id -g twitchdownload)"
  /usr/bin/setpriv \
    --reuid="$downloader_uid" --regid="$downloader_gid" --clear-groups \
    --no-new-privs --bounding-set=-all --inh-caps=-all --ambient-caps=-all \
    /usr/bin/env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin LC_ALL=C "$@"
}

ipv4_is_downloader_denied() {
  local address="$1"
  local first second third fourth
  IFS='.' read -r first second third fourth <<<"$address"
  if [[ ! "$first" =~ ^[0-9]+$ || ! "$second" =~ ^[0-9]+$ ||
        ! "$third" =~ ^[0-9]+$ || ! "$fourth" =~ ^[0-9]+$ ]] ||
     ((10#$first > 255 || 10#$second > 255 || 10#$third > 255 || 10#$fourth > 255)); then
    return 1
  fi
  if ((10#$first == 0 || 10#$first == 10 || 10#$first == 127 || 10#$first >= 224)) ||
     ((10#$first == 100 && 10#$second >= 64 && 10#$second <= 127)) ||
     ((10#$first == 169 && 10#$second == 254)) ||
     ((10#$first == 172 && 10#$second >= 16 && 10#$second <= 31)) ||
     ((10#$first == 192 && (10#$second == 0 || 10#$second == 168))) ||
     ((10#$first == 192 && 10#$second == 31 && 10#$third == 196)) ||
     ((10#$first == 192 && 10#$second == 52 && 10#$third == 193)) ||
     ((10#$first == 192 && 10#$second == 88 && 10#$third == 99)) ||
     ((10#$first == 192 && 10#$second == 175 && 10#$third == 48)) ||
     ((10#$first == 198 && (10#$second == 18 || 10#$second == 19))) ||
     ((10#$first == 198 && 10#$second == 51 && 10#$third == 100)) ||
     ((10#$first == 203 && 10#$second == 0 && 10#$third == 113)); then
    return 0
  fi
  return 1
}

probe_downloader_denied_listener() {
  local bind_address="$1"
  local label="$2"
  local control_port="$3"
  local denied_port="$4"
  local listener_pid

  /usr/bin/nc -l "$bind_address" "$control_port" >/dev/null 2>&1 &
  listener_pid=$!
  /usr/bin/sleep 0.05
  if ! kill -0 "$listener_pid" 2>/dev/null ||
     ! /usr/bin/nc -z -w 2 "$bind_address" "$control_port"; then
    kill "$listener_pid" 2>/dev/null || true
    wait "$listener_pid" 2>/dev/null || true
    echo "Kontrollverbindung für Downloader-$label-Egressprobe ist fehlgeschlagen." >&2
    return 1
  fi
  wait "$listener_pid" 2>/dev/null || true

  /usr/bin/nc -l "$bind_address" "$denied_port" >/dev/null 2>&1 &
  listener_pid=$!
  /usr/bin/sleep 0.05
  if ! kill -0 "$listener_pid" 2>/dev/null; then
    wait "$listener_pid" 2>/dev/null || true
    echo "Listener für Downloader-$label-Egressprobe konnte nicht starten." >&2
    return 1
  fi
  if run_as_downloader /usr/bin/nc -z -w 2 "$bind_address" "$denied_port"; then
    kill "$listener_pid" 2>/dev/null || true
    wait "$listener_pid" 2>/dev/null || true
    echo "Downloader erreicht trotz nft-Gate ein $label-Ziel: $bind_address" >&2
    return 1
  fi
  if ! kill -0 "$listener_pid" 2>/dev/null; then
    wait "$listener_pid" 2>/dev/null || true
    echo "Downloader-$label-Egressprobe hat den Listener unerwartet erreicht." >&2
    return 1
  fi
  kill "$listener_pid" 2>/dev/null || true
  wait "$listener_pid" 2>/dev/null || true
}

install_and_probe_downloader_egress() {
  local ruleset_file="$1"
  local validator="$2"
  local private_address='' twitch_ipv4='' dns_output=''
  local candidate_cidr candidate_address address_index interface_name address_family remainder

  "$validator" --before-load
  "$NFT_PATH" --file "$ruleset_file"
  "$validator" --after-load

  probe_downloader_denied_listener 127.0.0.1 Loopback 45873 45874
  while read -r address_index interface_name address_family candidate_cidr remainder; do
    : "$address_index" "$interface_name" "$address_family" "$remainder"
    candidate_address="${candidate_cidr%/*}"
    if ipv4_is_downloader_denied "$candidate_address" &&
       [[ "$candidate_address" != 127.* ]]; then
      private_address="$candidate_address"
      break
    fi
  done < <(/usr/bin/ip -o -4 address show scope global)
  if [[ -z "$private_address" ]]; then
    echo "Für die echte Downloader-Privatnetzprobe fehlt eine lokale private IPv4-Adresse." >&2
    return 1
  fi
  probe_downloader_denied_listener "$private_address" Privatnetz 45875 45876

  dns_output="$(run_as_downloader /usr/bin/dig \
    +time=5 +tries=1 +short @1.1.1.1 www.twitch.tv A)" || {
    echo "Downloader erreicht den fest erlaubten öffentlichen UDP-DNS-Resolver nicht." >&2
    return 1
  }
  if ! run_as_downloader /usr/bin/dig \
      +tcp +time=5 +tries=1 +short @1.1.1.1 www.twitch.tv A >/dev/null; then
    echo "Downloader erreicht den fest erlaubten öffentlichen TCP-DNS-Resolver nicht." >&2
    return 1
  fi
  while IFS= read -r candidate_address; do
    if [[ "$candidate_address" =~ ^([0-9]{1,3}\.){3}[0-9]{1,3}$ ]] &&
       ! ipv4_is_downloader_denied "$candidate_address"; then
      twitch_ipv4="$candidate_address"
      break
    fi
  done <<<"$dns_output"
  if [[ -z "$twitch_ipv4" ]]; then
    echo "Öffentlicher Resolver liefert keine zulässige Twitch-IPv4-Adresse." >&2
    return 1
  fi
  if ! run_as_downloader /usr/bin/curl \
      --silent --show-error --output /dev/null --connect-timeout 10 --max-time 20 \
      --proto '=http' --resolve "www.twitch.tv:80:$twitch_ipv4" \
      http://www.twitch.tv/; then
    echo "Downloader erreicht Twitch nicht über den erlaubten öffentlichen HTTP-Egress." >&2
    return 1
  fi
  if ! run_as_downloader /usr/bin/curl \
      --silent --show-error --output /dev/null --connect-timeout 10 --max-time 20 \
      --proto '=https' --tlsv1.2 --resolve "www.twitch.tv:443:$twitch_ipv4" \
      https://www.twitch.tv/; then
    echo "Downloader erreicht Twitch nicht über den erlaubten öffentlichen HTTPS-Egress." >&2
    return 1
  fi
}

install_release_system_unit() {
  local release_root="$1"
  local relative_source="$2"
  local unit_name="$3"
  local unit_dir="${4:-/etc/systemd/system}"
  local expected_uid="${5:-0}"
  local expected_gid="${6:-0}"
  local source_file="$release_root/$relative_source"
  local target_file="$unit_dir/$unit_name"
  local staged_unit

  if [[ ! "$unit_name" =~ ^[A-Za-z0-9_.@-]+$ ]] ||
     [[ "$unit_dir" != /* ]]; then
    echo "Ungültiges Systemd-Unitziel für die atomare Installation." >&2
    return 1
  fi
  if [[ ! -d "$unit_dir" || -L "$unit_dir" ]] ||
     [[ "$(stat -c '%u:%g' -- "$unit_dir")" != "$expected_uid:$expected_gid" ]] ||
     [[ -n "$(find "$unit_dir" -maxdepth 0 -perm /022 -print -quit)" ]]; then
    echo "Systemd-Systemverzeichnis ist für atomare Unitinstallation nicht vertrauenswürdig." >&2
    return 1
  fi
  validate_no_symlink_components "$unit_dir"
  if [[ -L "$source_file" || ! -f "$source_file" ]] ||
     [[ "$(stat -c '%u:%g:%h' -- "$source_file")" != \
        "$expected_uid:$expected_gid:1" ]] ||
     [[ -n "$(find "$source_file" -maxdepth 0 -perm /022 -print -quit)" ]]; then
    echo "Release-Unit ist nicht root-eigen, regulär und unveränderlich: $relative_source" >&2
    return 1
  fi
  validate_no_symlink_components "$source_file"
  if [[ -e "$target_file" || -L "$target_file" ]]; then
    if [[ -L "$target_file" || ! -f "$target_file" ]] ||
       [[ "$(stat -c '%u:%g:%h' -- "$target_file")" != \
          "$expected_uid:$expected_gid:1" ]] ||
       [[ -n "$(find "$target_file" -maxdepth 0 -perm /022 -print -quit)" ]]; then
      echo "Bestehendes Systemd-Unitziel ist kein vertrauenswürdiges reguläres Root-Ziel: $target_file" >&2
      return 1
    fi
  fi

  staged_unit="$(/usr/bin/mktemp "$unit_dir/.${unit_name}.install-XXXXXXXX")"
  if ! /usr/bin/install -o "$expected_uid" -g "$expected_gid" -m 0644 \
      -- "$source_file" "$staged_unit"; then
    /usr/bin/unlink -- "$staged_unit" 2>/dev/null || true
    return 1
  fi
  if ! /usr/bin/mv -Tf -- "$staged_unit" "$target_file"; then
    /usr/bin/unlink -- "$staged_unit" 2>/dev/null || true
    return 1
  fi
}

harden_runtime_media_tree() {
  local group_record passwd_records account
  for account in twitchbot twitchdash; do
    if ! /usr/bin/getent passwd "$account" >/dev/null; then
      echo "Laufzeitkonto fehlt für Medienrechte: $account" >&2
      return 1
    fi
    if /usr/bin/pgrep -u "$account" >/dev/null; then
      echo "Medienrechte werden nur bei vollständig gestopptem $account gesetzt." >&2
      return 1
    fi
  done
  if ! group_record="$(/usr/bin/getent group twitchmedia)"; then
    echo "Mediengruppe twitchmedia fehlt oder hat nicht die erwarteten Laufzeitkonten." >&2
    return 1
  fi
  passwd_records="$(/usr/bin/getent passwd)"
  validate_media_group_record "$group_record" "$passwd_records"
  probe_bwrap_fd_sandbox twitchbot
  probe_bwrap_fd_sandbox twitchdash
  harden_media_tree \
    /var/lib/deadlock-twitch-media twitchbot twitchdash twitchmedia root
}

# Hermetischer Prüfpfad für Build-Gate und Regressionstest. Er benötigt keine
# Root-Rechte und verändert weder Checkout noch Releasebaum.
if [[ "${1:-}" == "--validate-yt-dlp-artifact" ]]; then
  if [[ $# -ne 3 ]]; then
    echo "Aufruf: $0 --validate-yt-dlp-artifact <pfad> <prüfsummenmanifest>" >&2
    exit 1
  fi
  expected_checksum="$(read_yt_dlp_checksum_file "$3")"
  validate_yt_dlp_artifact "$2" "$expected_checksum"
  exit 0
fi

# Nicht privilegierter Regressionseinstieg. Das Produktionsziel ist fest in
# `harden_runtime_media_tree` verdrahtet und kann über diesen Pfad nicht
# gewählt werden.
if [[ "${1:-}" == "--harden-media-tree-for-test" ]]; then
  if [[ $# -ne 4 ]]; then
    echo "Interner Testaufruf ist unvollständig." >&2
    exit 1
  fi
  if [[ $EUID -eq 0 ]]; then
    echo "Der Medienbaum-Testmodus darf niemals mit Root-Rechten laufen." >&2
    exit 1
  fi
  if [[ "$3" != "$EUID" ]] ||
     [[ " $(/usr/bin/id -G) " != *" $4 "* ]]; then
    echo "Der Medienbaum-Testmodus darf nur die eigene Testidentität verwenden." >&2
    exit 1
  fi
  harden_media_tree "$2" "$3" "$3" "$4" "$3"
  exit 0
fi
if [[ "${1:-}" == "--validate-media-group-for-test" ]]; then
  if [[ $# -ne 3 ]]; then
    echo "Interner Gruppentestaufruf ist unvollständig." >&2
    exit 1
  fi
  validate_media_group_record "$2" "$3"
  exit 0
fi
if [[ "${1:-}" == "--validate-bwrap-for-test" ]]; then
  if [[ $# -ne 5 ]]; then
    echo "Interner Bubblewrap-Testaufruf ist unvollständig." >&2
    exit 1
  fi
  if [[ $EUID -eq 0 || "$4" != "$EUID" ]] ||
     [[ " $(/usr/bin/id -G) " != *" $5 "* ]]; then
    echo "Der Bubblewrap-Testmodus darf nur unprivilegiert mit der eigenen Identität laufen." >&2
    exit 1
  fi
  validate_bwrap_binary "$2" "$3" "$4" "$5"
  exit 0
fi
if [[ "${1:-}" == "--validate-system-bwrap-for-test" ]]; then
  if [[ $# -ne 1 || $EUID -eq 0 ]]; then
    echo "Der feste Bubblewrap-Systemtest darf nur unprivilegiert laufen." >&2
    exit 1
  fi
  validate_bwrap_binary "$BWRAP_PATH" "$BWRAP_MIN_VERSION" 0 0
  exit 0
fi
if [[ "${1:-}" == "--validate-nft-for-test" ]]; then
  if [[ $# -ne 5 || $EUID -eq 0 || "$4" != "$EUID" ]] ||
     [[ " $(/usr/bin/id -G) " != *" $5 "* ]]; then
    echo "Der nftables-Testmodus darf nur unprivilegiert mit der eigenen Identität laufen." >&2
    exit 1
  fi
  validate_nft_binary "$2" "$3" "$4" "$5"
  exit 0
fi
if [[ "${1:-}" == "--validate-public-resolver-for-test" ]]; then
  if [[ $# -ne 4 || $EUID -eq 0 || "$3" != "$EUID" ]] ||
     [[ " $(/usr/bin/id -G) " != *" $4 "* ]]; then
    echo "Der Resolver-Testmodus darf nur unprivilegiert mit der eigenen Identität laufen." >&2
    exit 1
  fi
  validate_public_resolver_file "$2" "$3" "$4"
  exit 0
fi
if [[ "${1:-}" == "--validate-downloader-account-for-test" ]]; then
  if [[ $# -ne 5 || $EUID -eq 0 ]]; then
    echo "Der Downloader-Kontentest darf nur unprivilegiert laufen." >&2
    exit 1
  fi
  validate_downloader_account_records "$2" "$3" "$4" "$5"
  exit 0
fi
if [[ "${1:-}" == "--install-system-unit-for-test" ]]; then
  if [[ $# -ne 7 || $EUID -eq 0 || "$6" != "$EUID" ]] ||
     [[ " $(/usr/bin/id -G) " != *" $7 "* ]]; then
    echo "Der Systemd-Unit-Testmodus darf nur unprivilegiert mit der eigenen Identität laufen." >&2
    exit 1
  fi
  install_release_system_unit "$2" "$3" "$4" "$5" "$6" "$7"
  exit 0
fi

if [[ $EUID -ne 0 ]]; then
  echo "Der Release-Installer muss als root laufen." >&2
  exit 1
fi
if [[ $# -ne 2 ]]; then
  echo "Aufruf: $0 <sauberer-checkout> <vollständiger-git-sha>" >&2
  exit 1
fi
validate_bwrap_binary "$BWRAP_PATH" "$BWRAP_MIN_VERSION" 0 0
validate_nft_binary "$NFT_PATH" "$NFT_MIN_VERSION" 0 0
validate_nft_package_state
validate_downloader_runtime_account
validate_offline_unit_gates

checkout="$(realpath -- "$1")"
git_sha="$2"
if [[ ! "$git_sha" =~ ^[0-9a-f]{40}$ ]]; then
  echo "Ungültiger Git-SHA: erwartet werden 40 kleine Hexzeichen." >&2
  exit 1
fi
build_root=/opt/deadlock/twitch/builds
if [[ "$(dirname -- "$checkout")" != "$build_root" ]]; then
  echo "Der Build muss vor dem Installieren unter $build_root eingefroren werden." >&2
  exit 1
fi
if [[ "$(basename -- "$checkout")" != "$git_sha" ]]; then
  echo "Das Build-Verzeichnis muss exakt den vollständigen Git-SHA tragen." >&2
  exit 1
fi
if [[ "$(stat -c '%U:%G' -- "$build_root")" != "root:root" ]] ||
   [[ -n "$(find "$build_root" -maxdepth 0 -perm /022 -print -quit)" ]]; then
  echo "Build-Wurzel ist nicht root-eigen und schreibgeschützt: $build_root" >&2
  exit 1
fi

git_dir="$checkout/.git"
if [[ ! -d "$git_dir" || -L "$git_dir" ]]; then
  echo "Der Release-Checkout muss ein eigenständiger Clone mit internem .git-Verzeichnis sein." >&2
  exit 1
fi
unsafe_git_entry="$(find "$git_dir" -xdev ! \( -type f -o -type d \) -print -quit)"
if [[ -n "$unsafe_git_entry" ]]; then
  echo "Git-Metadaten enthalten einen Symlink oder Sonderdateityp: $unsafe_git_entry" >&2
  exit 1
fi
unsafe_checkout="$(find "$checkout" -xdev \( ! -user root -o \( ! -type l -perm /022 \) \) -print -quit)"
if [[ -n "$unsafe_checkout" ]]; then
  echo "Checkout oder Git-Metadaten sind nicht vollständig eingefroren: $unsafe_checkout" >&2
  exit 1
fi
if [[ -f "$git_dir/objects/info/alternates" ]] ||
   [[ -e "$git_dir/info/attributes" || -L "$git_dir/info/attributes" ]]; then
  echo "Externe Git-Objekte oder unverfolgte Git-Attribute sind für Releases nicht erlaubt." >&2
  exit 1
fi

# Git erhält eine vollständige Allowlist-Umgebung. Ersetzungsobjekte könnten
# sonst selbst einen expliziten SHA transparent umbiegen.
git_safe=(
  /usr/bin/env -i
  PATH=/usr/sbin:/usr/bin:/sbin:/bin
  LC_ALL=C
  GIT_CONFIG_NOSYSTEM=1
  GIT_CONFIG_GLOBAL=/dev/null
  GIT_NO_REPLACE_OBJECTS=1
  GIT_OPTIONAL_LOCKS=0
  GIT_NO_LAZY_FETCH=1
  GIT_ATTR_NOSYSTEM=1
  /usr/bin/git
  -c core.fsmonitor=false
  -c core.hooksPath=/dev/null
  -c core.attributesFile=/dev/null
  -c core.sshCommand=/usr/bin/false
  -c credential.helper=
  -c protocol.allow=never
)
if [[ "$("${git_safe[@]}" -C "$checkout" rev-parse --absolute-git-dir)" != "$git_dir" ]] ||
   [[ "$("${git_safe[@]}" -C "$checkout" rev-parse HEAD)" != "$git_sha" ]]; then
  echo "Checkout und angegebener Git-SHA stimmen nicht überein." >&2
  exit 1
fi
if ! yt_dlp_checksum_line="$("${git_safe[@]}" -C "$checkout" show "$git_sha:$YT_DLP_CHECKSUM_PATH")"; then
  echo "Das gepinnte yt-dlp-Prüfsummenmanifest fehlt im angegebenen Git-SHA." >&2
  exit 1
fi
expected_yt_dlp_checksum="$(parse_yt_dlp_checksum_line "$yt_dlp_checksum_line")"
# Kein `git status` als root: lokale Filter/Attribute des zuvor unprivilegierten
# Builders könnten dabei Programme ausführen. Vertrauenswürdige Skripte und SQL
# kommen unten ausschließlich per `git archive` aus dem expliziten SHA;
# generierte Artefakte werden separat auf Typ, Eigentum und Schreibschutz geprüft.

generated=(
  rust/target/release/tb-bot
  rust/target/release/tb-dashboard
  rust/target/release/tb-stream-audit
  "$DOWNLOADER_RELEASE_PATH"
  "$YT_DLP_RELEASE_PATH"
  bot/analytics/dashboard_v2/dist
  bot/admin_dashboard/dist
  website/dist
)
validate_yt_dlp_artifact "$checkout/$YT_DLP_RELEASE_PATH" "$expected_yt_dlp_checksum"
for relative in "${generated[@]}"; do
  if [[ ! -e "$checkout/$relative" ]]; then
    echo "Release-Artefakt fehlt: $relative" >&2
    exit 1
  fi
  validate_no_symlink_components "$checkout/$relative"
  unsafe_entry="$(find "$checkout/$relative" -xdev ! \( -type f -o -type d \) -print -quit)"
  if [[ -n "$unsafe_entry" ]]; then
    echo "Release-Artefakt enthält einen Symlink oder Sonderdateityp: $unsafe_entry" >&2
    exit 1
  fi
  unsafe_owner="$(find "$checkout/$relative" -xdev \( ! -user root -o -perm /022 \) -print -quit)"
  if [[ -n "$unsafe_owner" ]]; then
    echo "Release-Artefakt ist nicht eingefroren: $unsafe_owner" >&2
    exit 1
  fi
done

# Der Rechtewechsel ist Bestandteil jedes Releases. Er läuft erst nach der
# vollständigen Artefaktprüfung und scheitert, solange Bot oder Dashboard noch
# einen Prozess besitzen.
harden_runtime_media_tree

release_root=/opt/deadlock/twitch/releases
install -d -o root -g root -m 0755 "$release_root"
release="$release_root/$git_sha"
if [[ ! -e "$release" ]]; then
  stage="$(mktemp -d "$release_root/.stage-$git_sha-XXXXXXXX")"
  cleanup_stage() {
    if [[ -d "$stage" && "$stage" == "$release_root"/.stage-"$git_sha"-* ]]; then
      find "$stage" -xdev -depth -delete
    fi
  }
  trap cleanup_stage EXIT

  install -d -m 0755 \
    "$stage/rust/target/release" \
    "$stage/rust/scripts" \
    "$stage/rust/migrations" \
    "$stage/rust/knowledge" \
    "$stage/ops/systemd" \
    "$stage/bot/analytics/dashboard_v2/dist" \
    "$stage/bot/admin_dashboard/dist" \
    "$stage/website/dist" \
    "$stage/data/clips" \
    "$stage/logs"

  install -m 0755 "$checkout/rust/target/release/tb-bot" "$stage/rust/target/release/tb-bot"
  install -m 0755 "$checkout/rust/target/release/tb-dashboard" "$stage/rust/target/release/tb-dashboard"
  install -m 0755 "$checkout/rust/target/release/tb-stream-audit" "$stage/rust/target/release/tb-stream-audit"
  install -m 0755 "$checkout/$DOWNLOADER_RELEASE_PATH" "$stage/$DOWNLOADER_RELEASE_PATH"
  install -m 0755 "$checkout/$YT_DLP_RELEASE_PATH" "$stage/$YT_DLP_RELEASE_PATH"
  validate_yt_dlp_artifact "$stage/$YT_DLP_RELEASE_PATH" "$expected_yt_dlp_checksum"

  # Skripte, Migrationen, Units und Rollen-SQL kommen direkt aus dem Git-Objekt
  # des angegebenen SHA. Unversionierte Dateien aus dem Build-Baum werden
  # niemals mit postgres-, root- oder Dienstrechten ausgeführt.
  archived_roots=(
    rust/scripts/run_tb_bot_service.sh
    rust/scripts/run_tb_dashboard_service.sh
    rust/scripts/run_stream_audit_service.sh
    rust/migrations
    rust/knowledge
    ops/systemd/deadlock-twitch-bot-rust.service
    ops/systemd/deadlock-twitch-dashboard-rust.service
    "$DOWNLOADER_SOCKET_PATH"
    "$DOWNLOADER_SERVICE_PATH"
    "$DOWNLOADER_FIREWALL_PATH"
    "$DOWNLOADER_FIREWALL_REFRESH_PATH"
    "$DOWNLOADER_FIREWALL_VALIDATOR_PATH"
    "$DOWNLOADER_RESOLVER_PATH"
    "$DOWNLOADER_NFT_PATH"
    ops/systemd/twitch-runtime-roles.sql
    "$YT_DLP_CHECKSUM_PATH"
  )
  "${git_safe[@]}" -C "$checkout" archive --format=tar "$git_sha" -- \
    "${archived_roots[@]}" \
    | /usr/bin/env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin LC_ALL=C \
      /usr/bin/tar --extract --file=- --directory="$stage" \
      --no-same-owner --no-same-permissions

  # Vor jedem root-seitigen chmod/chown müssen archivierte Quellen echte
  # reguläre Dateien sein. Ein getrackter Symlink dürfte sonst beim chmod sein
  # Ziel außerhalb des Releases verändern. Außerdem muss der extrahierte
  # Umfang exakt dem Git-Baum entsprechen; export-ignore darf nichts verbergen.
  unsafe_archived="$({
    cd "$stage"
    find "${archived_roots[@]}" -xdev ! \( -type f -o -type d \) -print -quit
  })"
  if [[ -n "$unsafe_archived" ]]; then
    echo "Archivierte Release-Quelle ist keine reguläre Datei: $unsafe_archived" >&2
    exit 1
  fi
  for required_file in \
    rust/scripts/run_tb_bot_service.sh \
    rust/scripts/run_tb_dashboard_service.sh \
    rust/scripts/run_stream_audit_service.sh \
    ops/systemd/deadlock-twitch-bot-rust.service \
    ops/systemd/deadlock-twitch-dashboard-rust.service \
    "$DOWNLOADER_SOCKET_PATH" \
    "$DOWNLOADER_SERVICE_PATH" \
    "$DOWNLOADER_FIREWALL_PATH" \
    "$DOWNLOADER_FIREWALL_REFRESH_PATH" \
    "$DOWNLOADER_FIREWALL_VALIDATOR_PATH" \
    "$DOWNLOADER_RESOLVER_PATH" \
    "$DOWNLOADER_NFT_PATH" \
    ops/systemd/twitch-runtime-roles.sql \
    "$YT_DLP_CHECKSUM_PATH"; do
    if [[ ! -f "$stage/$required_file" || -L "$stage/$required_file" ]]; then
      echo "Erforderliche Release-Quelle fehlt oder ist ein Symlink: $required_file" >&2
      exit 1
    fi
  done
  expected_archive=
  actual_archive=
  cleanup_archive_lists() {
    if [[ -n "$expected_archive" ]]; then
      unlink -- "$expected_archive" 2>/dev/null || true
    fi
    if [[ -n "$actual_archive" ]]; then
      unlink -- "$actual_archive" 2>/dev/null || true
    fi
  }
  trap 'cleanup_archive_lists; cleanup_stage' EXIT
  expected_archive="$(mktemp "$release_root/.expected-$git_sha-XXXXXXXX")"
  actual_archive="$(mktemp "$release_root/.actual-$git_sha-XXXXXXXX")"
  "${git_safe[@]}" -C "$checkout" ls-tree -rz --name-only "$git_sha" -- \
    "${archived_roots[@]}" | LC_ALL=C sort -z >"$expected_archive"
  (
    cd "$stage"
    find "${archived_roots[@]}" -type f -print0 | LC_ALL=C sort -z
  ) >"$actual_archive"
  if ! cmp --silent "$expected_archive" "$actual_archive"; then
    echo "Archivierter Release-Umfang stimmt nicht mit dem Git-SHA überein." >&2
    exit 1
  fi
  cleanup_archive_lists
  trap cleanup_stage EXIT

  chmod 0755 \
    "$stage/rust/scripts/run_tb_bot_service.sh" \
    "$stage/rust/scripts/run_tb_dashboard_service.sh" \
    "$stage/rust/scripts/run_stream_audit_service.sh" \
    "$stage/$DOWNLOADER_FIREWALL_VALIDATOR_PATH"
  chmod 0644 \
    "$stage/ops/systemd/deadlock-twitch-bot-rust.service" \
    "$stage/ops/systemd/deadlock-twitch-dashboard-rust.service" \
    "$stage/$DOWNLOADER_SOCKET_PATH" \
    "$stage/$DOWNLOADER_SERVICE_PATH" \
    "$stage/$DOWNLOADER_FIREWALL_PATH" \
    "$stage/$DOWNLOADER_FIREWALL_REFRESH_PATH" \
    "$stage/$DOWNLOADER_RESOLVER_PATH" \
    "$stage/$DOWNLOADER_NFT_PATH" \
    "$stage/ops/systemd/twitch-runtime-roles.sql" \
    "$stage/$YT_DLP_CHECKSUM_PATH"
  validate_public_resolver_file "$stage/$DOWNLOADER_RESOLVER_PATH" 0 0
  validate_downloader_nft_ruleset "$stage/$DOWNLOADER_NFT_PATH" 0 0
  cp -a "$checkout/bot/analytics/dashboard_v2/dist/." "$stage/bot/analytics/dashboard_v2/dist/"
  cp -a "$checkout/bot/admin_dashboard/dist/." "$stage/bot/admin_dashboard/dist/"
  cp -a "$checkout/website/dist/." "$stage/website/dist/"

  chown -R root:root "$stage"
  chmod -R go-w "$stage"
  # mktemp legt die Stage-Wurzel mit 0700 an. Die Laufzeitnutzer brauchen
  # Traversal-Rechte, dürfen den root-eigenen Release-Baum aber nie ändern.
  chmod 0755 "$stage"
  if [[ -n "$(find "$stage" -xdev ! \( -type f -o -type d \) -print -quit)" ]]; then
    echo "Der erzeugte Release-Baum enthält einen Symlink oder Sonderdateityp." >&2
    exit 1
  fi
  (
    cd "$stage"
    # Die Ausgabedatei existiert durch die Shell-Umleitung bereits, bevor
    # `find` startet. Sie darf deshalb nicht ihren eigenen Vorzustand hashen.
    find . -type f ! -path ./SHA256SUMS -print0 \
      | sort -z \
      | xargs -0 sha256sum > SHA256SUMS
    sha256sum --check --strict SHA256SUMS >/dev/null
  )
  chmod 0644 "$stage/SHA256SUMS"
  mv -- "$stage" "$release"
  trap - EXIT
fi

if [[ ! -d "$release" ]] || [[ -L "$release" ]] ||
   [[ "$(stat -c '%U:%G' -- "$release")" != "root:root" ]] ||
   [[ -n "$(find "$release" -xdev \( ! -user root -o -perm /022 \) -print -quit)" ]] ||
   [[ -n "$(find "$release" -xdev ! \( -type f -o -type d \) -print -quit)" ]] ||
   [[ ! -f "$release/SHA256SUMS" ]]; then
  echo "Bestehender Release-Baum ist nicht vertrauenswürdig: $release" >&2
  exit 1
fi
(
  cd "$release"
  sha256sum --check --strict SHA256SUMS >/dev/null
)
release_yt_dlp_checksum="$(read_yt_dlp_checksum_file "$release/$YT_DLP_CHECKSUM_PATH")"
validate_yt_dlp_artifact "$release/$YT_DLP_RELEASE_PATH" "$release_yt_dlp_checksum"
validate_public_resolver_file "$release/$DOWNLOADER_RESOLVER_PATH" 0 0
validate_downloader_nft_ruleset "$release/$DOWNLOADER_NFT_PATH" 0 0
install_and_probe_downloader_egress \
  "$release/$DOWNLOADER_NFT_PATH" \
  "$release/$DOWNLOADER_FIREWALL_VALIDATOR_PATH"

# Alle systemweiten Units kommen ausschließlich aus dem bereits versiegelten
# Release. Solange noch eine Datei scheitert, sieht PID 1 keine Teilmenge, weil
# daemon-reload erst nach der letzten atomaren Ersetzung erfolgt. Dienste und
# Socket bleiben bis zum separaten Migrations-/Start-Gate gestoppt.
install_release_system_unit "$release" \
  ops/systemd/deadlock-twitch-bot-rust.service \
  deadlock-twitch-bot-rust.service
install_release_system_unit "$release" \
  ops/systemd/deadlock-twitch-dashboard-rust.service \
  deadlock-twitch-dashboard-rust.service
install_release_system_unit "$release" "$DOWNLOADER_SOCKET_PATH" \
  deadlock-twitch-media-downloader.socket
install_release_system_unit "$release" "$DOWNLOADER_SERVICE_PATH" \
  deadlock-twitch-media-downloader@.service
install_release_system_unit "$release" "$DOWNLOADER_FIREWALL_PATH" \
  deadlock-twitch-media-downloader-firewall.service
install_release_system_unit "$release" "$DOWNLOADER_FIREWALL_REFRESH_PATH" \
  deadlock-twitch-media-downloader-firewall-refresh@.service
/usr/bin/systemctl daemon-reload

current_tmp=/opt/deadlock/twitch/.current-next
if [[ -e "$current_tmp" || -L "$current_tmp" ]]; then
  echo "Temporärer Current-Link existiert bereits: $current_tmp" >&2
  exit 1
fi
ln -s "releases/$git_sha" "$current_tmp"
mv -Tf -- "$current_tmp" /opt/deadlock/twitch/current

echo "Twitch-Release aktiviert: $git_sha"
