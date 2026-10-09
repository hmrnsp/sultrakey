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

useradd -r -m lakupandai
mkdir -p /opt/lakupandai
chmod 755 /opt/lakupandai
cd /opt/lakupandai
printf '# Port aplikasi\n# @plain\nPORT=8899\n# Host Redis\nREDIS_HOST=\n# @optional\nREDIS_PASSWORD=\n' > .env.example

expect_exit 64 sultrakey init lakupandai
sultrakey init lakupandai --owner lakupandai
[ "$(stat -c '%a %U' /etc/sultrakey/lakupandai.key)" = "400 lakupandai" ]
[ "$(stat -c '%a %U' .env)" = "600 lakupandai" ]

printf 'REDIS_HOST=10.10.1.20\n' | sultrakey setup
[ "$(stat -c '%a %U' .env)" = "600 lakupandai" ]
grep -q '^REDIS_HOST=enc:' .env
runuser -u lakupandai -- sultrakey check --env /opt/lakupandai/.env
sultrakey list

# shellcheck disable=SC2016 # expanded by the application's shell, not this one
out=$(runuser -u lakupandai -- sultrakey run --env /opt/lakupandai/.env -- \
    sh -c 'echo "$REDIS_HOST|$PORT|[${REDIS_PASSWORD-unset}]|${SULTRAKEY_KEY_FILE-none}"')
[ "$out" = "10.10.1.20|8899|[]|none" ] || { echo "FAIL: run printed '$out'" >&2; exit 1; }

# exec: the application keeps sultrakey's PID.
out=$(runuser -u lakupandai -- sh -c 'echo $$; exec sultrakey run --env /opt/lakupandai/.env -- sh -c "echo \$\$"')
[ "$(echo "$out" | sed -n 1p)" = "$(echo "$out" | sed -n 2p)" ] || { echo "FAIL: PID changed: $out" >&2; exit 1; }

# A configuration problem stops with 78, and the application does not start.
sed -i 's/^REDIS_HOST=.*/REDIS_HOST=/' .env
expect_exit 78 runuser -u lakupandai -- sultrakey run --env /opt/lakupandai/.env -- sh -c 'echo started'

# A plain .env from before sultrakey is taken over and encrypted.
mkdir -p /opt/old && cd /opt/old
printf 'PORT=1\nDB_PASSWORD=rahasia\n' > .env
printf '# @plain\nPORT=\nDB_PASSWORD=\n' > .env.example
sultrakey init old --owner lakupandai
grep -q '^DB_PASSWORD=enc:' .env
if grep -q rahasia .env; then
    echo "FAIL: plain value left in .env" >&2
    exit 1
fi

echo "OK: CentOS 7 smoke test passed"
