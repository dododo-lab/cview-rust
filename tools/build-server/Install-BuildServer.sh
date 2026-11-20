#!/bin/bash
# Install the Ubuntu queue and its restricted SSH key after provisioning the toolchain.
set -euo pipefail
umask 022

if [[ $EUID -ne 0 || $# -ne 4 ]]; then
    echo "Usage: sudo bash $0 public-key-file allowed-ip cargo-xwin cargo-tauri" >&2
    exit 2
fi

key_file=$1
client_ip=$2
if ! python3 -c 'import ipaddress, sys; ipaddress.ip_address(sys.argv[1])' "$client_ip"; then
    echo 'Expected one trusted client IP address' >&2
    exit 2
fi
read -r key_type key_data _ < "$key_file"
if [[ $key_type != ssh-ed25519 || ! $key_data =~ ^[A-Za-z0-9+/]+={0,2}$ ]] ||
    ! ssh-keygen -lf "$key_file" >/dev/null; then
    echo 'Expected an Ed25519 public key' >&2
    exit 2
fi

account=cview-build
home=/srv/cview-build
tools=/usr/local/lib/cview-build
key_dir=/etc/ssh/cview-build
ssh_config=/etc/ssh/sshd_config.d/70-cview-build.conf
source_dir=$(cd "$(dirname "$0")" && pwd)

if ! getent passwd "$account" >/dev/null; then
    useradd --system --create-home --home-dir "$home" --shell /bin/sh "$account"
fi
if [[ $(getent passwd "$account" | cut -d: -f6) != "$home" ]]; then
    echo 'Unexpected home for the existing build account' >&2
    exit 1
fi
install -d -m 0700 -o "$account" -g "$account" "$home"
install -d -m 0755 -o root -g root "$tools/bin" "$key_dir"
install -m 0644 -o root -g root "$source_dir"/build_{common,gateway,runner}.py \
    "$source_dir/pack_windows_tests.py" "$source_dir/Build-Windows.sh" "$tools/"
install -m 0755 -o root -g root "$3" "$tools/bin/cargo-xwin"
install -m 0755 -o root -g root "$4" "$tools/bin/cargo-tauri"

sudo -u "$account" -H rustup set profile minimal
sudo -u "$account" -H rustup default stable
sudo -u "$account" -H rustup component add clippy rustfmt
sudo -u "$account" -H rustup target add x86_64-pc-windows-msvc aarch64-pc-windows-msvc
sudo -u "$account" -H /bin/bash "$tools/Build-Windows.sh" "$home" --check-only

key_line=$(printf 'restrict,from="%s",command="/usr/bin/python3 -E -s -B %s/build_gateway.py --root %s" %s %s\n' \
    "$client_ip" "$tools" "$home" "$key_type" "$key_data")
if [[ -f "$key_dir/authorized_keys" ]] && ! grep -Fxq "$key_line" "$key_dir/authorized_keys"; then
    echo 'Another build key is registered. Check authorized_keys before replacing it.' >&2
    exit 1
fi
printf '%s\n' "$key_line" > "$key_dir/authorized_keys"
chmod 0644 "$key_dir/authorized_keys"
if [[ ! -e "$ssh_config" ]]; then
    cat > "$ssh_config" <<EOF
Match User $account
    AuthorizedKeysFile $key_dir/authorized_keys
    AuthenticationMethods publickey
    DisableForwarding yes
    PermitTTY no
Match all
EOF
    if ! /usr/sbin/sshd -t; then
        rm "$ssh_config"
        exit 1
    fi
fi
/usr/sbin/sshd -t
systemctl reload ssh
