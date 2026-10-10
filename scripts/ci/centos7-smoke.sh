#!/bin/bash
# Runs inside a centos:7 container (see .github/workflows/ci.yml): the static musl binary
# must install and work on the oldest supported server, the way infra uses it.
#   docker run --rm -v "$PWD/sultrakey:/sultrakey:ro" -v "$PWD/scripts/ci:/ci:ro" centos:7 bash /ci/centos7-smoke.sh
set -euo pipefail

expect_exit() {
    local want=$1
    shift
    set +e
    "$@"
    local got=$?
    set -e
    if [ "$got" -ne "$want" ]; then
        echo "FAIL: '$*' exited $got, expected $want" >&2
        exit 1
    fi
}

cat /etc/centos-release
cp /sultrakey /tmp/sultrakey
/tmp/sultrakey install
test -x /usr/bin/sultrakey
test -d /etc/sultrakey
sultrakey --version

useradd -r -m example-app
mkdir -p /opt/example-app
chmod 755 /opt/example-app
cd /opt/example-app
printf '# Port aplikasi\n# @plain\nPORT=8899\n# Host Redis\nREDIS_HOST=\n# @optional\nREDIS_PASSWORD=\n' > .env.example

expect_exit 64 sultrakey init example-app
sultrakey init example-app --owner example-app
[ "$(stat -c '%a %U' /etc/sultrakey/example-app.key)" = "400 example-app" ]
[ "$(stat -c '%a %U' .env)" = "600 example-app" ]

printf 'REDIS_HOST=10.10.1.20\n' | sultrakey setup
[ "$(stat -c '%a %U' .env)" = "600 example-app" ]
grep -q '^REDIS_HOST=enc:' .env
runuser -u example-app -- sultrakey check --env /opt/example-app/.env
sultrakey list

# shellcheck disable=SC2016 # expanded by the application's shell, not this one
out=$(runuser -u example-app -- sultrakey run --env /opt/example-app/.env -- \
    sh -c 'echo "$REDIS_HOST|$PORT|[${REDIS_PASSWORD-unset}]|${SULTRAKEY_KEY_FILE-none}"')
[ "$out" = "10.10.1.20|8899|[]|none" ] || { echo "FAIL: run printed '$out'" >&2; exit 1; }

# exec: the application keeps sultrakey's PID.
out=$(runuser -u example-app -- sh -c 'echo $$; exec sultrakey run --env /opt/example-app/.env -- sh -c "echo \$\$"')
[ "$(echo "$out" | sed -n 1p)" = "$(echo "$out" | sed -n 2p)" ] || { echo "FAIL: PID changed: $out" >&2; exit 1; }

# A configuration problem stops with 78, and the application does not start.
sed -i 's/^REDIS_HOST=.*/REDIS_HOST=/' .env
expect_exit 78 runuser -u example-app -- sultrakey run --env /opt/example-app/.env -- sh -c 'echo started'

# A plain .env from before sultrakey is taken over and encrypted.
mkdir -p /opt/old && cd /opt/old
printf 'PORT=1\nDB_PASSWORD=rahasia\n' > .env
printf '# @plain\nPORT=\nDB_PASSWORD=\n' > .env.example
sultrakey init old --owner example-app
grep -q '^DB_PASSWORD=enc:' .env
if grep -q rahasia .env; then
    echo "FAIL: plain value left in .env" >&2
    exit 1
fi

echo "OK: CentOS 7 smoke test passed"
