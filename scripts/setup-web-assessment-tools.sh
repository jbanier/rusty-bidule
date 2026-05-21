#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/setup-web-assessment-tools.sh [--apt-only] [--dry-run] [--install-dir DIR]

Install Ubuntu tooling used by the bundled web application posture recipes.

Options:
  --apt-only        Only install packages available through apt.
  --dry-run         Print commands without running them.
  --install-dir DIR Directory for copied/symlinked upstream binaries.
                    Defaults to /usr/local/bin.

Notes:
  Ubuntu apt does not reliably package every tool used by the recipes. By
  default this script installs apt prerequisites, then uses upstream installers
  for ProjectDiscovery tools, wscat, WPScan, and testssl.sh when needed.
  OWASP ZAP and Burp Suite are intentionally not installed here; the recipes
  treat them as operator-assisted tools.
EOF
}

APT_ONLY=0
DRY_RUN=0
INSTALL_DIR="/usr/local/bin"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --apt-only)
      APT_ONLY=1
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --install-dir)
      if [[ $# -lt 2 ]]; then
        echo "missing value for --install-dir" >&2
        exit 2
      fi
      INSTALL_DIR="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -r /etc/os-release ]]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  if [[ "${ID:-}" != "ubuntu" ]]; then
    echo "warning: this script assumes Ubuntu; detected ID=${ID:-unknown}" >&2
  fi
fi

if ! command -v apt-get >/dev/null 2>&1; then
  echo "apt-get is required" >&2
  exit 1
fi

run() {
  printf '+'
  printf ' %q' "$@"
  printf '\n'
  if [[ "$DRY_RUN" -eq 0 ]]; then
    "$@"
  fi
}

run_root() {
  if [[ "${EUID:-$(id -u)}" -eq 0 ]]; then
    run "$@"
  else
    run sudo "$@"
  fi
}

command_exists() {
  command -v "$1" >/dev/null 2>&1
}

apt_has_package() {
  apt-cache show "$1" >/dev/null 2>&1
}

APT_PACKAGES=(
  ca-certificates
  curl
  git
  python3
  tar
  unzip
  build-essential
  pkg-config
  libpcap-dev
  golang-go
  nodejs
  npm
  ruby-full
  ruby-dev
  zlib1g-dev
  libcurl4-openssl-dev
  libxml2-dev
  libxslt1-dev
  nmap
  wafw00f
  testssl.sh
  ffuf
  feroxbuster
  wpscan
  websocat
  whois
  dnsutils
)

echo "Installing apt packages..."
run_root apt-get update

AVAILABLE_APT_PACKAGES=()
UNAVAILABLE_APT_PACKAGES=()
if [[ "$DRY_RUN" -eq 1 ]]; then
  AVAILABLE_APT_PACKAGES=("${APT_PACKAGES[@]}")
else
  for package in "${APT_PACKAGES[@]}"; do
    if apt_has_package "$package"; then
      AVAILABLE_APT_PACKAGES+=("$package")
    else
      UNAVAILABLE_APT_PACKAGES+=("$package")
    fi
  done
fi

if [[ "${#AVAILABLE_APT_PACKAGES[@]}" -gt 0 ]]; then
  run_root apt-get install -y --no-install-recommends "${AVAILABLE_APT_PACKAGES[@]}"
fi

if [[ "${#UNAVAILABLE_APT_PACKAGES[@]}" -gt 0 ]]; then
  echo "Apt packages not available on this Ubuntu release: ${UNAVAILABLE_APT_PACKAGES[*]}" >&2
fi

if [[ "$APT_ONLY" -eq 1 ]]; then
  echo "Apt-only mode complete. Some tools may still be missing if Ubuntu does not package them." >&2
  exit 0
fi

run_root install -d -m 0755 "$INSTALL_DIR"

install_go_tool() {
  local binary="$1"
  local module="$2"

  if command_exists "$binary"; then
    echo "$binary is already installed: $(command -v "$binary")"
    return
  fi

  if ! command_exists go; then
    echo "go is required to install $binary from $module" >&2
    return 1
  fi

  local gopath
  gopath="$(go env GOPATH)"
  run go install "$module"
  if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "dry-run: would copy $gopath/bin/$binary to $INSTALL_DIR/$binary"
    return
  fi
  if [[ ! -x "$gopath/bin/$binary" ]]; then
    echo "expected $gopath/bin/$binary after go install $module" >&2
    return 1
  fi
  run_root install -m 0755 "$gopath/bin/$binary" "$INSTALL_DIR/$binary"
}

install_projectdiscovery_tools() {
  install_go_tool httpx github.com/projectdiscovery/httpx/cmd/httpx@latest
  install_go_tool subfinder github.com/projectdiscovery/subfinder/v2/cmd/subfinder@latest
  install_go_tool dnsx github.com/projectdiscovery/dnsx/cmd/dnsx@latest
  install_go_tool naabu github.com/projectdiscovery/naabu/v2/cmd/naabu@latest
  install_go_tool nuclei github.com/projectdiscovery/nuclei/v3/cmd/nuclei@latest
  install_go_tool katana github.com/projectdiscovery/katana/cmd/katana@latest
}

install_extra_go_tools() {
  install_go_tool dalfox github.com/hahwul/dalfox/v2@latest
  install_go_tool gospider github.com/jaeles-project/gospider@latest
  install_go_tool ffuf github.com/ffuf/ffuf/v2@latest
}

install_wscat() {
  if command_exists wscat; then
    echo "wscat is already installed: $(command -v wscat)"
    return
  fi
  if ! command_exists npm; then
    echo "npm is required to install wscat" >&2
    return 1
  fi
  run_root npm install -g wscat
}

install_wpscan() {
  if command_exists wpscan; then
    echo "wpscan is already installed: $(command -v wpscan)"
    return
  fi
  if ! command_exists gem; then
    echo "gem is required to install wpscan" >&2
    return 1
  fi
  run_root gem install wpscan
}

install_testssl() {
  if command_exists testssl.sh; then
    echo "testssl.sh is already installed: $(command -v testssl.sh)"
    return
  fi
  if ! command_exists git; then
    echo "git is required to install testssl.sh from upstream" >&2
    return 1
  fi

  local dest="/opt/testssl.sh"
  if [[ "$DRY_RUN" -eq 1 ]]; then
    echo "dry-run: would clone or update https://github.com/drwetter/testssl.sh.git at $dest"
    echo "dry-run: would symlink $dest/testssl.sh to $INSTALL_DIR/testssl.sh"
    return
  fi

  if [[ -d "$dest/.git" ]]; then
    run_root git -C "$dest" pull --ff-only
  else
    run_root git clone --depth 1 https://github.com/drwetter/testssl.sh.git "$dest"
  fi
  run_root ln -sf "$dest/testssl.sh" "$INSTALL_DIR/testssl.sh"
}

install_projectdiscovery_tools
install_extra_go_tools
install_wscat
install_wpscan
install_testssl

cat <<EOF

Installation complete.

Expected web assessment binaries:
  curl nmap wafw00f testssl.sh httpx subfinder dnsx naabu nuclei katana
  ffuf feroxbuster dalfox wpscan wscat websocat gospider

Missing binaries, if any:
EOF

for binary in curl nmap wafw00f testssl.sh httpx subfinder dnsx naabu nuclei katana ffuf feroxbuster dalfox wpscan wscat websocat gospider; do
  if command_exists "$binary"; then
    printf '  ok      %s -> %s\n' "$binary" "$(command -v "$binary")"
  else
    printf '  missing %s\n' "$binary"
  fi
done
