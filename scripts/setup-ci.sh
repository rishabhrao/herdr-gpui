#!/usr/bin/env bash
# Install what `just ci release-check check-commits test-build audit-deps
# lint-windows` needs on a developer machine, so a local run can match CI.
# macOS uses Homebrew; Linux uses apt (Ubuntu 24.04, like the CI runners) and
# falls back to Cargo or a pinned, checksum-verified download where apt has no
# package. Safe to rerun: installed tools are kept, missing ones are added.
#
# Docker, for `just lint-linux`, is only checked: installing a container
# runtime is the developer's choice.
set -euo pipefail

fail() { printf '%s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

root=$(git rev-parse --show-toplevel)
tools=${HERDR_CI_TOOLS:-${XDG_DATA_HOME:-$HOME/.local/share}/herdr-gpui/ci-tools}
git_cliff_version=2.12.0 # Matches the release workflow's changelog job.
# Tools pinned into $tools count as installed even before the developer adds
# the directory to PATH, so reruns never download them again.
user_path=$PATH
export PATH="$tools/bin:$PATH"

# The Linux release archive's SHA-256 is copied from actionlint's
# checksums.txt for this exact version; bump them together.
install_actionlint() {
    local version=1.7.12 asset sha tmp
    case "$(uname -m)" in
        x86_64) asset=linux_amd64 sha=8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8 ;;
        aarch64) asset=linux_arm64 sha=325e971b6ba9bfa504672e29be93c24981eeb1c07576d730e9f7c8805afff0c6 ;;
        *) fail "No pinned actionlint build for $(uname -m)" ;;
    esac
    tmp=$(mktemp -d)
    curl -fsSL --proto '=https' --tlsv1.2 -o "$tmp/actionlint.tar.gz" \
        "https://github.com/rhysd/actionlint/releases/download/v$version/actionlint_${version}_$asset.tar.gz"
    echo "$sha  $tmp/actionlint.tar.gz" | sha256sum --check --quiet || fail 'actionlint archive digest mismatch'
    tar -xzf "$tmp/actionlint.tar.gz" -C "$tmp" actionlint
    mkdir -p "$tools/bin"
    install -m 0755 "$tmp/actionlint" "$tools/bin/actionlint"
    rm -rf -- "$tmp"
}

have rustup || fail 'rustup is required: https://rustup.rs'
# The pinned toolchain from rust-toolchain.toml, as CI installs it, plus the
# GNU target that `just lint-windows` cross-checks. Cargo installs below need it.
(cd "$root" && rustup toolchain install && rustup target add x86_64-pc-windows-gnu wasm32-unknown-unknown)

case "$(uname -s)" in
    Darwin)
        have brew || fail 'Homebrew is required: https://brew.sh'
        packages=()
        have actionlint || packages+=(actionlint)
        have zizmor || packages+=(zizmor)
        have cargo-deny || packages+=(cargo-deny)
        have git-cliff || packages+=(git-cliff)
        have jq || packages+=(jq)
        have x86_64-w64-mingw32-gcc || packages+=(mingw-w64)
        # Apple's /usr/bin/python3 is 3.9; the release script tests need 3.11+.
        python3 -c 'import sys; sys.exit(sys.version_info < (3, 11))' 2>/dev/null || packages+=(python)
        if ((${#packages[@]})); then
            brew install "${packages[@]}"
            # Forget the python3 resolved by the version check above.
            hash -r
        fi
        ;;
    Linux)
        bash "$root/scripts/install-linux-deps.sh"
        if [[ $EUID -eq 0 ]]; then sudo=(); else sudo=(sudo); fi
        "${sudo[@]}" apt-get install --no-install-recommends -y gcc-mingw-w64-x86-64 rsync curl
        have actionlint || install_actionlint
        have zizmor || cargo install --locked zizmor
        have cargo-deny || cargo install --locked cargo-deny
        have wasm-bindgen || cargo install --locked wasm-bindgen-cli --version 0.2.128
        have git-cliff || cargo install --locked git-cliff --version "$git_cliff_version"
        ;;
    *)
        # Native Windows CI runs only format, lint and tests, which need
        # nothing beyond the pinned toolchain below.
        fail "Unsupported host $(uname -s); on Windows, rustup show is all CI needs"
        ;;
esac

# Lets the release script tests build real Linux packages, as CI does. The
# installer owns the pin, so read the version from it rather than repeat it.
nfpm=$tools/bin/nfpm
nfpm_version=$(sed -n 's/^version=//p' "$root/scripts/release/install-nfpm.sh")
if ! { [[ -x $nfpm ]] && "$nfpm" --version 2>/dev/null | grep -F "$nfpm_version" >/dev/null; }; then
    nfpm=$(bash "$root/scripts/release/install-nfpm.sh" "$tools/bin")
fi

missing=()
for tool in cargo actionlint zizmor cargo-deny git-cliff jq x86_64-w64-mingw32-gcc; do
    have "$tool" || missing+=("$tool")
done
python3 -c 'import sys; sys.exit(sys.version_info < (3, 11))' || missing+=('python3 >= 3.11 first on PATH')
have docker || printf 'note: docker not found; `just lint-linux` needs it\n' >&2

# %q keeps the printed commands correct for paths with spaces or quotes.
if [[ $(uname -s) == Linux && ":$user_path:" != *":$tools/bin:"* ]]; then
    printf 'Add the pinned tools to PATH:\n  export PATH=%q:"$PATH"\n' "$tools/bin"
fi
printf 'To build real .deb/.rpm/Arch packages in `just release-check`:\n  export HERDR_TEST_NFPM=%q\n' "$nfpm"
((${#missing[@]} == 0)) || fail "Still missing: ${missing[*]}"
echo 'CI tools ready.'
