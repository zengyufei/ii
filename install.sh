#!/usr/bin/env bash

set -o pipefail

REPOSITORY="${II_REPOSITORY:-zengyufei/ii}"
API_URL="${II_RELEASE_API_URL:-https://api.github.com/repos/${REPOSITORY}/releases/latest}"
INSTALL_DIR="${II_INSTALL_DIR:-${HOME}/.local/bin}"

error() {
  printf 'ii install: error: %s\n' "$*" >&2
  return 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || error "required command not found: $1"
}

detect_asset() {
  local os="$1"
  local arch="$2"

  case "${os}/${arch}" in
    Linux/x86_64 | Linux/amd64) printf '%s\n' 'ii-linux-x86_64' ;;
    Darwin/arm64 | Darwin/aarch64) printf '%s\n' 'ii-macos-aarch64' ;;
    Linux/*) error "unsupported Linux architecture: ${arch} (only x86_64 is available)" ;;
    Darwin/*) error "unsupported macOS architecture: ${arch} (only Apple Silicon is available)" ;;
    *) error "unsupported operating system: ${os} (only Linux and macOS are available)" ;;
  esac
}

release_tag() {
  local response tag

  response="$(curl --fail --location --silent --show-error "$API_URL")" || {
    error "could not query the latest ii release"
    return 1
  }
  tag="$(printf '%s' "$response" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  if [[ ! "$tag" =~ ^[A-Za-z0-9._-]+$ ]]; then
    error "latest ii release did not contain a usable tag"
    return 1
  fi
  printf '%s\n' "$tag"
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

verify_checksum() {
  local asset="$1"
  local binary="$2"
  local checksums="$3"
  local expected actual

  expected="$(awk -v asset="$asset" '$2 == asset { print $1; exit }' "$checksums")"
  if [[ ! "$expected" =~ ^[[:xdigit:]]{64}$ ]]; then
    error "release checksums do not contain ${asset}"
    return 1
  fi
  actual="$(sha256_file "$binary")" || {
    error "could not calculate SHA-256 for ${asset}"
    return 1
  }
  if [[ "$actual" != "$expected" ]]; then
    error "SHA-256 mismatch for ${asset}"
    return 1
  fi
}

print_path_hint() {
  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) return 0 ;;
  esac

  local profile
  case "${SHELL:-}" in
    */bash) profile="${HOME}/.bashrc" ;;
    */zsh) profile="${HOME}/.zshrc" ;;
    *) profile="your shell profile" ;;
  esac

  printf '\nii install: %s is not in PATH. Add this to %s:\n' "$INSTALL_DIR" "$profile"
  printf '  export PATH="%s:$PATH"\n' "$INSTALL_DIR"
}

main() {
  local asset tag download_base tmp_dir binary checksums

  require_command curl || return 1
  require_command awk || return 1
  require_command sed || return 1
  require_command mktemp || return 1
  require_command install || return 1
  if ! command -v sha256sum >/dev/null 2>&1 && ! command -v shasum >/dev/null 2>&1; then
    error "required command not found: sha256sum or shasum"
    return 1
  fi

  asset="$(detect_asset "$(uname -s)" "$(uname -m)")" || return 1
  tag="$(release_tag)" || return 1
  download_base="https://github.com/${REPOSITORY}/releases/download/${tag}"
  tmp_dir="$(mktemp -d)" || {
    error "could not create a temporary directory"
    return 1
  }
  trap 'rm -rf "$tmp_dir"' EXIT
  binary="${tmp_dir}/${asset}"
  checksums="${tmp_dir}/checksums.txt"

  printf 'ii install: downloading %s (%s)\n' "$tag" "$asset"
  curl --fail --location --silent --show-error --output "$binary" "${download_base}/${asset}" || {
    error "could not download ${asset} from ${tag}"
    return 1
  }
  curl --fail --location --silent --show-error --output "$checksums" "${download_base}/checksums.txt" || {
    error "could not download checksums.txt from ${tag}"
    return 1
  }
  verify_checksum "$asset" "$binary" "$checksums" || return 1

  mkdir -p "$INSTALL_DIR" || {
    error "could not create install directory ${INSTALL_DIR}"
    return 1
  }
  install -m 755 "$binary" "${INSTALL_DIR}/ii" || {
    error "could not install ii to ${INSTALL_DIR}/ii"
    return 1
  }
  printf 'ii install: installed %s\n' "${INSTALL_DIR}/ii"
  print_path_hint
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
