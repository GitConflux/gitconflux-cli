#!/bin/sh
# GitConflux CLI installer for macOS and Linux.
#
#   curl --disable --proto '=https' --proto-redir '=https' --tlsv1.2 -fsSL \
#     https://raw.githubusercontent.com/GitConflux/gitconflux-cli/main/scripts/install.sh | sh
#
# Environment:
#   GCFLX_VERSION       Stable tag vMAJOR.MINOR.PATCH. Default: latest.
#   GCFLX_INSTALL_DIR   Install directory. Default: $HOME/.local/bin.
#   GCFLX_DOWNLOAD_BASE Override the release download base URL (https:// or file://).
#
# Requires GitHub CLI (gh). Verifies the release checksum and signed GitHub
# provenance before extracting, and only installs the single expected binary.

set -eu

REPO="GitConflux/gitconflux-cli"
BIN="gcflx"

say() { printf '%s\n' "$*"; }
fail() { printf 'error: %s\n' "$*" >&2; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || fail "required command not found: $1"; }

fetch() {
    # $1 = url, $2 = output file
    curl --disable --globoff --proto '=https,file' --proto-redir '=https' \
        --tlsv1.2 --fail --silent --show-error --location --retry 3 --output "$2" -- "$1"
}

detect_target() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$os" in
        Darwin) os_part="apple-darwin" ;;
        Linux) os_part="unknown-linux-gnu" ;;
        *) fail "unsupported operating system: $os (use the Windows installer on Windows)" ;;
    esac
    case "$arch" in
        x86_64 | amd64) arch_part="x86_64" ;;
        arm64 | aarch64) arch_part="aarch64" ;;
        *) fail "unsupported CPU architecture: $arch" ;;
    esac
    printf '%s-%s' "$arch_part" "$os_part"
}

latest_version() {
    # Follow the /releases/latest redirect instead of the rate-limited REST API.
    url=$(curl --disable --proto '=https' --proto-redir '=https' \
        --tlsv1.2 --fail --silent --show-error --location \
        --output /dev/null --write-out '%{url_effective}' \
        "https://github.com/$REPO/releases/latest") || fail "could not determine the latest release"
    prefix="https://github.com/$REPO/releases/tag/"
    case "$url" in
        "$prefix"*) printf '%s' "${url#"$prefix"}" ;;
        *) fail "no published release found at https://github.com/$REPO/releases" ;;
    esac
}

valid_version() {
    # Keep in sync with the release workflow. The character check also rejects newlines.
    case "$1" in
        *[!v0123456789.]*) return 1 ;;
    esac
    [ "${#1}" -le 64 ] && printf '%s\n' "$1" |
        LC_ALL=C grep -Exq 'v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)'
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        fail "need sha256sum or shasum to verify the download"
    fi
}

main() {
    need curl
    need tar
    need uname
    need mktemp

    version=${GCFLX_VERSION:-}
    [ -n "$version" ] || version=$(latest_version)
    valid_version "$version" || fail "invalid release tag (expected vMAJOR.MINOR.PATCH, at most 64 characters)"
    command -v gh >/dev/null 2>&1 || fail "install or update GitHub CLI (gh) from https://cli.github.com/"

    target=$(detect_target)
    name="$BIN-$version-$target"
    base=${GCFLX_DOWNLOAD_BASE:-"https://github.com/$REPO/releases/download/$version"}
    case "$base" in
        https://* | file://*) ;;
        *) fail "GCFLX_DOWNLOAD_BASE must start with https:// or file://" ;;
    esac

    install_dir=${GCFLX_INSTALL_DIR:-"${HOME:?HOME is not set}/.local/bin"}

    tmp=$(mktemp -d "${TMPDIR:-/tmp}/gcflx-install.XXXXXX")
    trap 'rm -rf "$tmp"' EXIT INT TERM

    say "Installing $BIN $version for $target"
    fetch "$base/$name.tar.gz" "$tmp/$name.tar.gz" || fail "archive download failed"
    fetch "$base/$name.tar.gz.sha256" "$tmp/$name.tar.gz.sha256" \
        || fail "checksum download failed"

    expected=$(cut -d ' ' -f 1 "$tmp/$name.tar.gz.sha256")
    actual=$(sha256_of "$tmp/$name.tar.gz")
    case "$expected" in
        *[!0-9a-f]* | "") fail "malformed checksum file" ;;
    esac
    [ "${#expected}" -eq 64 ] || fail "malformed checksum file"
    [ "$expected" = "$actual" ] || fail "checksum mismatch (expected $expected, got $actual)"

    bundle="$BIN-$version.sigstore.json"
    fetch "$base/$bundle" "$tmp/$bundle" || fail "release provenance download failed"
    ref="refs/tags/$version"
    GH_PROMPT_DISABLED=1 gh attestation verify "$tmp/$name.tar.gz" \
        --bundle "$tmp/$bundle" --hostname github.com --repo "$REPO" --source-ref "$ref" \
        --cert-identity "https://github.com/$REPO/.github/workflows/release.yml@$ref" \
        --deny-self-hosted-runners || fail "release provenance verification failed"

    # Extract only the one member we expect, into the private temp directory.
    mkdir "$tmp/extract"
    tar -xzf "$tmp/$name.tar.gz" -C "$tmp/extract" "$name/$BIN" \
        || fail "archive does not contain $name/$BIN"
    [ -f "$tmp/extract/$name/$BIN" ] && [ ! -L "$tmp/extract/$name/$BIN" ] \
        || fail "unexpected archive contents"

    mkdir -p "$install_dir"
    # Write to a temp name in the destination, then rename for an atomic replace.
    cp "$tmp/extract/$name/$BIN" "$install_dir/.$BIN.new.$$"
    chmod 755 "$install_dir/.$BIN.new.$$"
    mv -f "$install_dir/.$BIN.new.$$" "$install_dir/$BIN"

    say "Installed $install_dir/$BIN"
    case ":$PATH:" in
        *":$install_dir:"*) ;;
        *) say "Note: $install_dir is not on your PATH. Add this to your shell profile:"
           say "  export PATH=\"$install_dir:\$PATH\"" ;;
    esac
    say "Run '$BIN --help' to get started, then '$BIN auth login'."
}

main "$@"
