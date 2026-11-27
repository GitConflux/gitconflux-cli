#!/bin/sh
# Offline installer test with a local release fixture and a stub provenance verifier.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/gcflx-itest.XXXXXX")
trap 'rm -rf "$work"' EXIT

case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) target=aarch64-apple-darwin ;;
    Darwin-x86_64) target=x86_64-apple-darwin ;;
    Linux-x86_64) target=x86_64-unknown-linux-gnu ;;
    Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-gnu ;;
    *) echo "unsupported host"; exit 1 ;;
esac

version=v9.9.9
name="gcflx-$version-$target"
rel="$work/rel"
mkdir -p "$rel/$name"
printf '#!/bin/sh\necho fake-gcflx\n' > "$rel/$name/gcflx"
chmod 755 "$rel/$name/gcflx"
(cd "$rel" && tar czf "$name.tar.gz" "$name")
write_checksum() (
    cd "$rel"
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$name.tar.gz" > "$name.tar.gz.sha256"
    else
        shasum -a 256 "$name.tar.gz" > "$name.tar.gz.sha256"
    fi
)
write_checksum

cp "$rel/$name.tar.gz" "$work/pristine.tar.gz"
printf 'fixture bundle\n' > "$work/pristine.json"
cp "$work/pristine.json" "$rel/gcflx-$version.sigstore.json"
fixture_url="file://$(printf '%s' "$rel" | sed 's/%/%25/g; s/ /%20/g; s/#/%23/g; s/?/%3F/g')"

mkdir "$work/tools"
TEST_WORK=$work TEST_VERSION=$version TEST_ARCHIVE="$name.tar.gz"
TEST_TAR=$(command -v tar) TEST_CURL=$(command -v curl)
export TEST_WORK TEST_VERSION TEST_ARCHIVE TEST_TAR TEST_CURL
cat > "$work/tools/gh" <<'EOF'
#!/bin/sh
set -eu
touch "$TEST_WORK/verify-called"
repo=GitConflux/gitconflux-cli
ref="refs/tags/$TEST_VERSION"
[ "$#" -eq 14 ]
[ "$1 $2" = 'attestation verify' ]
[ "${3##*/}" = "$TEST_ARCHIVE" ]
[ "$4" = '--bundle' ] && [ "${5##*/}" = "gcflx-$TEST_VERSION.sigstore.json" ]
[ "$6 $7" = '--hostname github.com' ]
[ "$8 $9" = "--repo $repo" ]
[ "${10} ${11}" = "--source-ref $ref" ]
[ "${12} ${13}" = "--cert-identity https://github.com/$repo/.github/workflows/release.yml@$ref" ]
[ "${14}" = '--deny-self-hosted-runners' ]
cmp -s "$3" "$TEST_WORK/pristine.tar.gz"
cmp -s "$5" "$TEST_WORK/pristine.json"
touch "$TEST_WORK/verified"
EOF
cat > "$work/tools/tar" <<'EOF'
#!/bin/sh
set -eu
touch "$TEST_WORK/extract-called"
[ -f "$TEST_WORK/verified" ]
exec "$TEST_TAR" "$@"
EOF
cat > "$work/tools/curl" <<'EOF'
#!/bin/sh
set -eu
for last do :; done
case "$last" in
    https://github.com/GitConflux/gitconflux-cli/releases/latest)
        printf '%s' "${TEST_LATEST_URL:?}"
        ;;
    file://*)
        touch "$TEST_WORK/download-called"
        exec "$TEST_CURL" "$@"
        ;;
    *) exit 1 ;;
esac
EOF
chmod +x "$work/tools/"*

run() (
    rm -f "$work/verify-called" "$work/verified" "$work/extract-called" "$work/download-called"
    PATH="$work/tools:$PATH" GCFLX_VERSION="$1" \
        GCFLX_DOWNLOAD_BASE="${2:-$fixture_url}" GCFLX_INSTALL_DIR="$work/bin" \
        sh "$here/install.sh"
)

reject() {
    message=$1
    shift
    if run "$@" >"$work/output" 2>&1; then
        echo "FAIL: expected $message"; exit 1
    fi
    grep -Fq "$message" "$work/output" || { cat "$work/output"; exit 1; }
    [ ! -e "$work/extract-called" ] || { echo "FAIL: extraction attempted"; exit 1; }
    [ ! -e "$work/bin/gcflx" ] || { echo "FAIL: binary installed"; exit 1; }
}

# Verify successful latest-version resolution and verification before extraction.
export TEST_LATEST_URL="https://github.com/GitConflux/gitconflux-cli/releases/tag/$version"
run "" >/dev/null
[ "$("$work/bin/gcflx")" = "fake-gcflx" ] || { echo "FAIL: binary did not run"; exit 1; }
rm -rf "${work:?}/bin"

# A checksum alone cannot authorize a download.
printf 'changed bytes' >> "$rel/$name.tar.gz"
reject 'checksum mismatch' "$version"
write_checksum
reject 'release provenance verification failed' "$version"
[ -f "$work/verify-called" ]

cp "$work/pristine.tar.gz" "$rel/$name.tar.gz"
write_checksum
printf 'not a bundle\n' > "$rel/gcflx-$version.sigstore.json"
reject 'release provenance verification failed' "$version"
rm "$rel/gcflx-$version.sigstore.json"
reject 'release provenance download failed' "$version"
[ ! -e "$work/verify-called" ]

for bad in 'v1.2' 'v1.2.3-rc.1' 'v01.2.3' 'v1.2.3/extra' 'latest' \
    "$(printf 'v1.2.3\nv1.2.4')" "v$(printf '%064d' 1).2.3"; do
    reject 'invalid release tag' "$bad"
    [ ! -e "$work/download-called" ]
done

export TEST_LATEST_URL="https://github.com/example/other/releases/tag/$version"
reject 'no published release found' ""
[ ! -e "$work/download-called" ]
export TEST_LATEST_URL="https://github.com/GitConflux/gitconflux-cli/releases/tag/$version?unexpected"
reject 'invalid release tag' ""
[ ! -e "$work/download-called" ]

reject 'must start with https:// or file://' "$version" 'http://example.invalid'

echo "install.sh tests passed"
