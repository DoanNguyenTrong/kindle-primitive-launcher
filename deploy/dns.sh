#!/bin/sh
# Public DNS (Cloudflare 1.1.1.1 and Google 8.8.8.8) for Settings > SSH & DNS. The network's own settings are saved
# the first time, so Restore puts them back. The Kindle's system partition is writable only while the copy runs.
# The original resolver file is /mnt/us/resolv.conf.original; the DHCP script's original is in ../dns/.
#
#   dns.sh public | restore | status
set -u

ORIG=/mnt/us/resolv.conf.original
DHCP_ORIG=/mnt/us/launcher/dns/default.script.original
DHCP=/usr/share/udhcpc/default.script

rw() { mount -o remount,rw /; }
ro() { sync; mount -o remount,ro / 2>/dev/null; }

case "$1" in
    public)
        [ -d /mnt/us ] || { echo "not a Kindle: nothing changed"; exit 1; }
        if [ ! -f "$ORIG" ] && ! cp /etc/resolv.conf "$ORIG"; then echo "could not save the original DNS settings: nothing changed"; exit 1; fi
        printf 'nameserver 1.1.1.1\nnameserver 8.8.8.8\n' > /mnt/us/resolv.conf.new || exit 1
        rw || { rm -f /mnt/us/resolv.conf.new; echo "could not make / writable: nothing changed"; exit 1; }
        cp /mnt/us/resolv.conf.new /etc/resolv.conf
        cp /mnt/us/resolv.conf.new /tmp/resolv.conf
        mkdir -p /mnt/us/launcher/dns
        [ -f "$DHCP_ORIG" ] || cp "$DHCP" "$DHCP_ORIG"
        # Reconnecting Wi-Fi keeps the setting: the DHCP script asks for the same two servers.
        sed 's/for i in $dns ; do/for i in 1.1.1.1 8.8.8.8 ; do/' "$DHCP" > /tmp/default.script.new \
            && grep -q 'for i in 1.1.1.1 8.8.8.8' /tmp/default.script.new \
            && cat /tmp/default.script.new > "$DHCP"
        r=$?
        rm -f /tmp/default.script.new /mnt/us/resolv.conf.new
        ro
        [ $r -eq 0 ] && echo "public DNS on" || echo "could not change the DHCP script; DNS is on until the next reconnect"
        exit $r
        ;;
    restore)
        [ -f "$ORIG" ] || { echo "no saved original DNS settings: nothing changed"; exit 1; }
        rw || { echo "could not make / writable: nothing changed"; exit 1; }
        cp "$ORIG" /etc/resolv.conf
        cp "$ORIG" /tmp/resolv.conf
        r=0
        if [ -f "$DHCP_ORIG" ]; then cp "$DHCP_ORIG" "$DHCP"; r=$?; fi
        ro
        [ $r -eq 0 ] && echo "network DNS restored" || echo "could not restore the DHCP script"
        exit $r
        ;;
    *)
        if grep -q '1.1.1.1' /etc/resolv.conf 2>/dev/null; then echo "public"; else echo "network"; fi
        ;;
esac
