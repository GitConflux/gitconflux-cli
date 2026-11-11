#!/bin/sh
# Offline test for install.sh: packages a fake release for this host and installs it via file://.
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
if command -v sha256sum >/dev/null 2>&1; then
    (cd "$rel" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
else
    (cd "$rel" && shasum -a 256 "$name.tar.gz" > "$name.tar.gz.sha256")
fi

run() {
    GCFLX_VERSION="$1" GCFLX_DOWNLOAD_BASE="file://$rel" GCFLX_INSTALL_DIR="$work/bin" \
        sh "$here/install.sh"
}

# 1. Happy path installs a working, executable binary.
run "$version" >/dev/null
[ "$("$work/bin/gcflx")" = "fake-gcflx" ] || { echo "FAIL: binary did not run"; exit 1; }

# 2. A tampered archive is rejected and nothing is replaced.
rm -rf "${work:?}/bin"
printf 'tampered' >> "$rel/$name.tar.gz"
if run "$version" >/dev/null 2>&1; then echo "FAIL: tampered archive accepted"; exit 1; fi
[ ! -e "$work/bin/gcflx" ] || { echo "FAIL: binary installed despite bad checksum"; exit 1; }

# 3. Hostile version strings are rejected before any download.
for bad in 'v1/../../x' 'v1;rm' 'latest'; do
    if GCFLX_VERSION="$bad" GCFLX_DOWNLOAD_BASE="file://$rel" GCFLX_INSTALL_DIR="$work/bin" \
        sh "$here/install.sh" >/dev/null 2>&1; then
        echo "FAIL: accepted version '$bad'"; exit 1
    fi
done

# 4. Plain-HTTP download base is rejected.
if GCFLX_VERSION="$version" GCFLX_DOWNLOAD_BASE="http://example.com" GCFLX_INSTALL_DIR="$work/bin" \
    sh "$here/install.sh" >/dev/null 2>&1; then
    echo "FAIL: http base accepted"; exit 1
fi

echo "install.sh tests passed"
