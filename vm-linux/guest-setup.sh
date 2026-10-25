#!/bin/sh
# Run in the Linux test VM by vm.sh, as the session's user, on every start
# and restart: points the session at the binaries shared from the host, and
# lists the desktop's apps for the launcher.
set -eu

BIN=/patina/target/release
apps=$HOME/.local/share/applications
icons=$HOME/.local/share/icons/hicolor/scalable/apps
mkdir -p "$apps" "$icons"

# Settings and the Process Manager, as make install lists them, from the share.
for app in settings:com.sectorflabs.Settings processes:com.sectorflabs.ProcessManager; do
    dir=${app%%:*} id=${app#*:}
    cp "/patina/$dir/packaging/$dir.svg" "$icons/$id.svg"
    sed "s|^Exec=patina-|Exec=$BIN/patina-|" "/patina/$dir/packaging/$id.desktop" > "$apps/$id.desktop"
done

# Teleprinter, the session's terminal, when its checkout is shared too.
tp=/teleprinter/packaging/linux
if [ -f /teleprinter/target/release/teleprinter ]; then
    cp "$tp/teleprinter.svg" "$icons/com.sectorflabs.Teleprinter.svg"
    sed "s|^Exec=teleprinter|Exec=/teleprinter/target/release/teleprinter|" \
        "$tp/com.sectorflabs.Teleprinter.desktop" > "$apps/com.sectorflabs.Teleprinter.desktop"
fi

# patina-session takes its environment from `sh -l` (ADR-0083), which reads
# this too, so starting the session is kept to a login on tty1.
cat > "$HOME/.profile" <<EOF
# Written by /patina/vm-linux/guest-setup.sh.
[ -f "\$HOME/.bashrc" ] && [ -n "\${BASH_VERSION:-}" ] && . "\$HOME/.bashrc"
PATH=$BIN:/teleprinter/target/release:\$PATH
export PATH
if [ "\$(tty)" = /dev/tty1 ] && [ -z "\${WAYLAND_DISPLAY:-}" ]; then
    exec dbus-run-session -- $BIN/patina-session > /tmp/patina-session.log 2>&1
fi
EOF
