#!/bin/sh
# Install each package in a clean container of its distro and check every library resolves.
# Arch: also builds packaging/aur/PKGBUILD from the local .deb (standing in for the release
# download) and refreshes packaging/aur/.SRCINFO.
set -eu
cd "$(dirname "$0")/.."
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
deb=target/debian/cadence_${ver}-1_amd64.deb
rpm=target/generate-rpm/cadence-${ver}-1.x86_64.rpm
check='ldd /usr/bin/cadence | grep "not found" && exit 1; echo "  all libraries found"; for c in crontab rsync notify-send pkexec; do command -v $c >/dev/null || { echo "  missing $c"; exit 1; }; done; echo "  crontab rsync notify-send pkexec present"'

echo "== Ubuntu 24.04 (.deb)"
docker run --rm -v "$PWD/target:/t:ro" ubuntu:24.04 sh -euc "
  apt-get update -qq >/dev/null && apt-get install -y -qq /t/debian/${deb##*/} >/dev/null 2>&1; $check"

echo "== Fedora (.rpm)"
docker run --rm -v "$PWD/target:/t:ro" fedora:latest sh -euc "
  dnf install -y -q /t/generate-rpm/${rpm##*/} >/dev/null 2>&1; $check"

echo "== openSUSE Tumbleweed (.rpm)"
docker run --rm -v "$PWD/target:/t:ro" opensuse/tumbleweed sh -euc "
  zypper -n -q --gpg-auto-import-keys install --allow-unsigned-rpm /t/generate-rpm/${rpm##*/} >/dev/null 2>&1; $check"

echo "== Arch (PKGBUILD)"
sum=$(sha256sum "$deb" | cut -d' ' -f1)
sed -i "s/^pkgver=.*/pkgver=$ver/; s/^pkgrel=.*/pkgrel=1/; s/^sha256sums=.*/sha256sums=('$sum')/" packaging/aur/PKGBUILD
W=target/arch
rm -rf "$W" && mkdir -p "$W"
cp packaging/aur/PKGBUILD packaging/aur/cadence.install "$deb" "$W/"
docker run --rm -v "$PWD/$W:/b" archlinux:latest sh -euc '
  pacman -Syu --noconfirm --needed base-devel namcap >/dev/null 2>&1
  useradd -m builder && chown -R builder /b && cd /b
  su builder -c "makepkg -f --noconfirm --nodeps >makepkg.log 2>&1 && makepkg --printsrcinfo >.SRCINFO" || { tail -20 makepkg.log; exit 1; }
  namcap PKGBUILD || true
  pacman -U --noconfirm cadence-bin-*.pkg.tar.zst 2>&1 | grep -i "cronie\|error" || true
  '"$check"'
  chown -R '"$(id -u):$(id -g)"' /b'
cp "$W/.SRCINFO" packaging/aur/.SRCINFO
ls -1 "$deb" "$rpm" "$W"/*.pkg.tar.zst
