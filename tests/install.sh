#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck source=../install.sh
source "${ROOT}/install.sh"

fail() {
  printf 'test failure: %s\n' "$*" >&2
  exit 1
}

assert_eq() {
  [[ "$1" == "$2" ]] || fail "expected '$1', got '$2'"
}

expect_failure() {
  if "$@" >/dev/null 2>&1; then
    fail "expected failure: $*"
  fi
}

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

assert_eq 'ii-linux-x86_64' "$(detect_asset Linux x86_64)"
assert_eq 'ii-linux-x86_64' "$(detect_asset Linux amd64)"
assert_eq 'ii-macos-aarch64' "$(detect_asset Darwin arm64)"
expect_failure detect_asset Linux aarch64
expect_failure detect_asset Darwin x86_64
expect_failure detect_asset FreeBSD x86_64

payload="${tmp_dir}/ii-linux-x86_64"
printf '#!/usr/bin/env bash\n[ "${1:-}" = version ] && printf "9.9.9\\n"\n' > "$payload"
chmod +x "$payload"
checksum="$(sha256_file "$payload")"
checksums="${tmp_dir}/checksums.txt"
printf '%s  %s\n' "$checksum" 'ii-linux-x86_64' > "$checksums"
verify_checksum 'ii-linux-x86_64' "$payload" "$checksums"
printf '%064d  %s\n' 0 'ii-linux-x86_64' > "$checksums"
expect_failure verify_checksum 'ii-linux-x86_64' "$payload" "$checksums"

fixture="${tmp_dir}/fixture"
mkdir -p "${fixture}/bin"
printf '{"tag_name":"v9.9.9"}\n' > "${fixture}/release.json"
printf '%s  %s\n' "$checksum" 'ii-linux-x86_64' > "${fixture}/checksums.txt"
cp "$payload" "${fixture}/ii-linux-x86_64"
cat > "${fixture}/bin/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

url="${!#}"
output=''
for ((index = 1; index <= $#; index += 1)); do
  if [[ "${!index}" == '--output' ]]; then
    next=$((index + 1))
    output="${!next}"
    break
  fi
done
case "${II_FIXTURE_MODE:-ok}:${url}" in
  api-fail:*) exit 22 ;;
  asset-fail:*ii-linux-x86_64) exit 22 ;;
  *:*/release) source_file="${II_FIXTURE_DIR}/release.json" ;;
  *:*/checksums.txt) source_file="${II_FIXTURE_DIR}/checksums.txt" ;;
  *:*/ii-linux-x86_64) source_file="${II_FIXTURE_DIR}/ii-linux-x86_64" ;;
  *) exit 22 ;;
esac
if [[ -n "$output" ]]; then
  cat "$source_file" > "$output"
else
  cat "$source_file"
fi
EOF
chmod +x "${fixture}/bin/curl"
cat > "${fixture}/bin/uname" <<'EOF'
#!/usr/bin/env bash

case "${1:-}" in
  -s) printf '%s\n' 'Linux' ;;
  -m) printf '%s\n' 'x86_64' ;;
  *) exit 1 ;;
esac
EOF
chmod +x "${fixture}/bin/uname"

run_install() {
  PATH="${fixture}/bin:${PATH}" \
    II_FIXTURE_DIR="$fixture" \
    II_REPOSITORY='fixture/ii' \
    II_RELEASE_API_URL='https://fixture.invalid/release' \
    II_INSTALL_DIR="$1" \
    bash "${ROOT}/install.sh"
}

run_default_install() {
  PATH="${fixture}/bin:${PATH}" \
    II_FIXTURE_DIR="$fixture" \
    II_REPOSITORY='fixture/ii' \
    II_RELEASE_API_URL='https://fixture.invalid/release' \
    HOME="$1" \
    bash "${ROOT}/install.sh"
}

default_home="${tmp_dir}/home"
run_default_install "$default_home" >/dev/null
assert_eq '9.9.9' "$("${default_home}/.local/bin/ii" version)"

custom_dir="${tmp_dir}/custom/bin"
run_install "$custom_dir" >/dev/null
assert_eq '9.9.9' "$("${custom_dir}/ii" version)"

printf '%064d  %s\n' 0 'ii-linux-x86_64' > "${fixture}/checksums.txt"
expect_failure run_install "${tmp_dir}/bad-checksum"
printf '%s  %s\n' "$checksum" 'ii-linux-x86_64' > "${fixture}/checksums.txt"
II_FIXTURE_MODE='api-fail' expect_failure run_install "${tmp_dir}/api-failure"
II_FIXTURE_MODE='asset-fail' expect_failure run_install "${tmp_dir}/asset-failure"

printf 'install.sh tests passed\n'
