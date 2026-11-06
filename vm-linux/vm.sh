#!/usr/bin/env bash
# The Linux test VM (ADR-0017): an Ubuntu cloud image of the host's release,
# booting straight into a Patina session on tty1, with the binaries built
# here shared in read-only. Its disk is an overlay in target/vm-linux, so a
# broken guest is one `reset` from fresh; the base image is cached outside the
# repo.
#
#   vm-linux/vm.sh start            create it on first use, boot it and wait for SSH
#                                 (PATINA_VM_INSTANCE=dev, the checkout and its builds, default;
#                                   PATINA_VM_INSTANCE=rc, the clean guest that installs the .deb)
#   vm-linux/vm.sh restart         set the guest up again and restart its session, after a build
#   vm-linux/vm.sh shot [png]      its screen, saved (default target/vm-linux/screen.png)
#   vm-linux/vm.sh ssh [cmd]       a shell (or a command) as the session's user
#   vm-linux/vm.sh fake ...        the guest's fake battery and Wi-Fi (fake-hardware.sh):
#                                 fake battery 15 discharging, fake ac off,
#                                 fake portal on (a captive portal on the fake Wi-Fi)
#   vm-linux/vm.sh view            its screen in a VNC viewer (localhost:5903)
#   vm-linux/vm.sh log             follow the serial console
#   vm-linux/vm.sh stop            power it off
#   vm-linux/vm.sh reset           stop it and delete its disk, for a fresh guest
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(dirname "$HERE")
INSTANCE=${PATINA_VM_INSTANCE:-dev}
OUT="$REPO/target/vm-$INSTANCE"
CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/patina-desktop"

# The guest is the host's release, so binaries built here run there unchanged.
. /etc/os-release
RELEASE=$VERSION_CODENAME
IMAGE_URL="https://cloud-images.ubuntu.com/$RELEASE/current/$RELEASE-server-cloudimg-amd64.img"
BASE="$CACHE/$RELEASE-server-cloudimg-amd64.img"

DISK="$OUT/disk.qcow2"
DISK_SIZE=20G
SEED="$OUT/seed"
KEY="$OUT/id_ed25519"
SSH_PORT=2224
VNC_DISPLAY=3
MONITOR="$OUT/monitor.sock"
PIDFILE="$OUT/qemu.pid"
SERIAL_LOG="$OUT/serial.log"
# Teleprinter, the session's terminal, from the checkout beside this one.
TELEPRINTER="${TELEPRINTER:-$(dirname "$REPO")/teleprinter}"

say() { printf '==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

running() { [[ -f "$PIDFILE" ]] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; }

guest() {
  ssh -i "$KEY" -p "$SSH_PORT" \
    -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR \
    patina@127.0.0.1 "$@"
}

create() {
  mkdir -p "$OUT" "$CACHE"
  if [[ ! -f "$BASE" ]]; then
    say "downloading $IMAGE_URL"
    curl -fL --progress-bar -o "$BASE.part" "$IMAGE_URL"
    mv "$BASE.part" "$BASE"
  fi
  [[ -f "$KEY" ]] || ssh-keygen -q -t ed25519 -N '' -C patina-vm-linux -f "$KEY"
  qemu-img create -q -f qcow2 -F qcow2 -b "$BASE" "$DISK" "$DISK_SIZE"
  # cloud-init's NoCloud seed, read from a folder QEMU shows as a FAT disk
  # labelled CIDATA.
  rm -rf "$SEED" && mkdir -p "$SEED"
  printf 'instance-id: patina-vm-linux-%s\nlocal-hostname: patina-vm\n' "$(date +%s)" > "$SEED/meta-data"
  sed "s|@KEY@|$(cat "$KEY.pub")|" "$HERE/user-data" > "$SEED/user-data"
  if [[ "$INSTANCE" == rc ]]; then
    # A clean guest installs the deb into its own disk, no shares: without a
    # source to mount, systemd's patina.mount unit fails, and the fake portal
    # that requires it can never start.
    awk 'BEGIN{skip=0} /^mounts:$/{skip=1} /^write_files:$/{skip=0} !skip' \
      "$SEED/user-data" > "$SEED/user-data.tmp" && mv "$SEED/user-data.tmp" "$SEED/user-data"
  fi
}

boot() {
  local shares=()
  if [[ "$INSTANCE" == dev ]]; then
    shares+=(-virtfs "local,path=$REPO,mount_tag=patina,security_model=none,readonly=on")
    if [[ -d "$TELEPRINTER" ]]; then
      shares+=(-virtfs "local,path=$TELEPRINTER,mount_tag=teleprinter,security_model=none,readonly=on")
    fi
  fi
  qemu-system-x86_64 \
    -enable-kvm -cpu host -m 4096 -smp 4 -machine q35 \
    -drive "file=$DISK,format=qcow2,if=virtio" \
    -drive "driver=vvfat,dir=$SEED,label=CIDATA,read-only=on,if=virtio" \
    "${shares[@]}" \
    -nic "user,model=virtio-net-pci,hostfwd=tcp:127.0.0.1:$SSH_PORT-:22" \
    -device virtio-vga,xres=1920,yres=1200 -vnc "127.0.0.1:$VNC_DISPLAY" \
    -device qemu-xhci -device usb-tablet -device usb-kbd \
    -monitor "unix:$MONITOR,server,nowait" \
    -display none -serial "file:$SERIAL_LOG" \
    -daemonize -pidfile "$PIDFILE"
}

wait_ssh() {
  # The first boot installs packages before SSH lets the user in.
  for _ in $(seq 1 300); do
    guest true 2>/dev/null && return 0
    running || die "QEMU exited; see $SERIAL_LOG"
    sleep 2
  done
  die "no SSH after 10 minutes; see $SERIAL_LOG"
}

setup() {
  if [[ "$INSTANCE" == dev ]]; then
    # Setting up waits for cloud-init's first run, then lays out what the
    # session needs from the shared checkout.
    guest 'cloud-init status --wait >/dev/null; sh /patina/vm-linux/guest-setup.sh'
    # A battery and Wi-Fi for the bar to read, as root.
    guest 'sudo sh /patina/vm-linux/fake-hardware.sh >/dev/null'
  else
    # rc: a clean guest with no shares: install the .deb, then the fake
    # hardware and the session from copies of the scripts beside it, with the
    # binaries the deb installed at /usr/bin.
    say "installing the deb into the clean rc guest"
    # First boot: let cloud-init finish its own packages, its additions to
    # /etc/apt, and the lists they bring, else the deb's dependencies read as
    # nobody's.
    guest 'sudo cloud-init status --wait'
    guest 'sudo apt-get update -qq'
    deb="$(ls -t "$REPO"/target/deb/*.deb | head -1)"
    scp -i "$KEY" -P "$SSH_PORT" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR "$deb" patina@127.0.0.1:/tmp/patina-desktop.deb
    guest 'DEBIAN_FRONTEND=noninteractive sudo apt-get install -y /tmp/patina-desktop.deb'
    scp -i "$KEY" -P "$SSH_PORT" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR "$HERE/guest-setup.sh" patina@127.0.0.1:/tmp/patina-guest-setup.sh
    scp -i "$KEY" -P "$SSH_PORT" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR "$HERE/fake-hardware.sh" patina@127.0.0.1:/tmp/patina-fake-hardware.sh
    scp -i "$KEY" -P "$SSH_PORT" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR "$HERE/fake-portal.py" patina@127.0.0.1:/tmp/patina-fake-portal.py
    guest 'BIN_DIR=/usr/bin sh /tmp/patina-guest-setup.sh'
    guest 'sudo PORTAL=/tmp/patina-fake-portal.py sh /tmp/patina-fake-hardware.sh >/dev/null'
  fi
}

case "${1:-}" in
  start)
    running && die "already running"
    [[ -f "$DISK" ]] || create
    boot
    say "booting; waiting for SSH (the first boot installs packages)"
    wait_ssh
    setup
    # tty1 logs in before the setup writes ~/.profile on a fresh guest.
    guest 'loginctl terminate-user patina' 2>/dev/null || true
    say "up: vm-linux/vm.sh view, or vm-linux/vm.sh shot"
    ;;
  restart)
    running || die "not running; vm-linux/vm.sh start"
    setup
    # Ending the user's login session takes the Patina session with it;
    # getty logs them in again on tty1, which starts it anew.
    guest 'loginctl terminate-user patina' 2>/dev/null || true
    say "session restarted"
    ;;
  shot)
    running || die "not running"
    shot=$(realpath -m "${2:-$OUT/screen.png}")
    rm -f "$shot"
    printf 'screendump %s -f png\n' "$shot" | nc -U -q 1 "$MONITOR" >/dev/null
    for _ in $(seq 1 25); do [[ -s "$shot" ]] && break; sleep 0.2; done
    [[ -s "$shot" ]] || die "no screenshot"
    echo "$shot"
    ;;
  ssh)
    shift
    guest "$@"
    ;;
  fake)
    running || die "not running"
    shift
    if [[ "$INSTANCE" == rc ]]; then
      fake_script=/tmp/patina-fake-hardware.sh
    else
      fake_script=/patina/vm-linux/fake-hardware.sh
    fi
    guest "sudo sh $fake_script $*"
    ;;
  view)
    running || die "not running"
    exec vncviewer "127.0.0.1:$((5900 + VNC_DISPLAY))"
    ;;
  log)
    exec tail -f "$SERIAL_LOG"
    ;;
  stop)
    running || exit 0
    kill "$(cat "$PIDFILE")"
    # Its ports stay taken until it has gone.
    for _ in $(seq 1 50); do running || break; sleep 0.2; done
    rm -f "$PIDFILE"
    say "stopped"
    ;;
  reset)
    "$0" stop
    rm -f "$DISK"
    rm -rf "$SEED"
    say "deleted the guest's disk; vm-linux/vm.sh start makes a fresh one"
    ;;
  *)
    sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
