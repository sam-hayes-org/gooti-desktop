#!/bin/bash
# Installs Ubuntu packages on a GitHub runner: tools/ci/apt-install.sh <package>...
#
# Two failures seen on the runners (2026-11-11):
# - The runner's DNS did not answer for *.ubuntu.com ("Temporary failure resolving"), while other
#   hosts worked. apt then prints "Ign:" every 20 seconds and a run hangs for an hour.
# - So the Azure mirror (azure.archive.ubuntu.com) may be unreachable while archive.ubuntu.com is not.
# Hence: when Ubuntu's names do not resolve, use public DNS before the first try. Each try has a time
# limit, and the second try uses public DNS and the main Ubuntu mirror.
set -euo pipefail

export DEBIAN_FRONTEND=noninteractive
APT=(apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=30)
TRY_SECONDS=360
PUBLIC_DNS=(1.1.1.1 8.8.8.8)
public_dns=false

try_install() {
  sudo -E timeout "$TRY_SECONDS" "${APT[@]}" update &&
    sudo -E timeout "$TRY_SECONDS" "${APT[@]}" install -y "$@"
}

resolves() {
  timeout 15 getent hosts "$1" >/dev/null
}

use_public_dns() {
  if [[ $public_dns == true ]]; then
    return
  fi
  public_dns=true
  local iface
  iface=$(ip route show default 2>/dev/null | awk '{print $5; exit}')
  if command -v resolvectl >/dev/null && [[ -n $iface ]]; then
    sudo resolvectl dns "$iface" "${PUBLIC_DNS[@]}"
    sudo resolvectl domain "$iface" '~.'
    sudo resolvectl flush-caches
  else
    printf 'nameserver %s\n' "${PUBLIC_DNS[@]}" | sudo tee /etc/resolv.conf >/dev/null
  fi
  if resolves archive.ubuntu.com; then
    echo "Public DNS resolves archive.ubuntu.com."
  else
    echo "::warning::Even public DNS does not resolve archive.ubuntu.com."
  fi
}

if ! resolves azure.archive.ubuntu.com && ! resolves archive.ubuntu.com; then
  echo "::warning::This runner cannot look up ubuntu.com names. Using public DNS (${PUBLIC_DNS[*]})."
  use_public_dns
fi

if try_install "$@"; then
  exit 0
fi

echo "::warning::The Azure Ubuntu mirror did not deliver. Trying archive.ubuntu.com with public DNS."
use_public_dns
# The runners' sources point at a mirror list (mirror+file:/etc/apt/apt-mirrors.txt) that names
# the Azure mirror. Older images name it in sources.list (22.04) or ubuntu.sources (24.04) directly.
for list in /etc/apt/apt-mirrors.txt /etc/apt/sources.list /etc/apt/sources.list.d/*.list /etc/apt/sources.list.d/*.sources; do
  if [[ -f $list ]]; then
    sudo sed -i 's#http://azure\.archive\.ubuntu\.com/ubuntu#http://archive.ubuntu.com/ubuntu#g' "$list"
  fi
done
try_install "$@"
