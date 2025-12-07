#!/usr/bin/env bash
#
# deploy_wanem_helper.sh - install the WAN tc/netem helper on a target system.
#
# This script copies the reference helper (wanem_helper.py) from the
# repository into a target prefix (default: /usr/local) and creates a
# small wrapper binary `tigervnc-wanem-helper` under $PREFIX/bin.
#
# After running this script (as root), you can configure your test
# environment with:
#
#   export TIGERVNC_WAN_HELPER="sudo -n ${PREFIX:-/usr/local}/bin/tigervnc-wanem-helper"
#
# and add an appropriate sudoers rule to allow passwordless execution of
# that helper for your user or group.
#
# WARNING: This script is intentionally minimal and assumes you are
# comfortable managing sudoers / capabilities on the target system.
# Review it before use.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="${SCRIPT_DIR%/tests/e2e}"

PREFIX="/usr/local"
BIN_DIR=""
LIB_DIR=""

print_usage() {
  cat <<EOF
Usage: $0 [--prefix DIR]

Options:
  --prefix DIR   Installation prefix (default: /usr/local)

This installs:
  - WAN helper  : \${PREFIX}/lib/tigervnc/wanem_helper.py
  - Wrapper     : \${PREFIX}/bin/tigervnc-wanem-helper

You should run this script as root (e.g. via sudo) on the target
machine.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help)
      print_usage
      exit 0
      ;;
    --prefix)
      shift
      if [[ $# -eq 0 ]]; then
        echo "ERROR: --prefix requires an argument" >&2
        exit 2
      fi
      PREFIX="$1"
      shift
      ;;
    --prefix=*)
      PREFIX="${1#*=}"
      shift
      ;;
    *)
      echo "ERROR: Unknown argument: $1" >&2
      print_usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "${PREFIX}" ]]; then
  echo "ERROR: PREFIX cannot be empty" >&2
  exit 2
fi

BIN_DIR="${PREFIX%/}/bin"
LIB_DIR="${PREFIX%/}/lib/tigervnc"

SRC_HELPER="${SCRIPT_DIR}/wanem_helper.py"
SRC_WANEM="${SCRIPT_DIR}/wanem.py"
if [[ ! -f "${SRC_HELPER}" ]]; then
  echo "ERROR: Source helper not found: ${SRC_HELPER}" >&2
  exit 1
fi
if [[ ! -f "${SRC_WANEM}" ]]; then
  echo "ERROR: Source WAN module not found: ${SRC_WANEM}" >&2
  exit 1
fi

echo "Installing WAN helper to:"
echo "  Helper : ${LIB_DIR}/wanem_helper.py"
echo "  Wrapper: ${BIN_DIR}/tigervnc-wanem-helper"

mkdir -p "${LIB_DIR}" "${BIN_DIR}"

cp "${SRC_HELPER}" "${LIB_DIR}/wanem_helper.py"
cp "${SRC_WANEM}" "${LIB_DIR}/wanem.py"
chmod 644 "${LIB_DIR}/wanem_helper.py" "${LIB_DIR}/wanem.py"

WRAPPER_TMP="${BIN_DIR}/tigervnc-wanem-helper.tmp.$$"
WRAPPER="${BIN_DIR}/tigervnc-wanem-helper"

cat > "${WRAPPER_TMP}" <<EOF
#!/usr/bin/env bash
set -euo pipefail
PY_HELPER="${LIB_DIR}/wanem_helper.py"
if [[ ! -f "\${PY_HELPER}" ]]; then
  echo "ERROR: WAN helper not found at \${PY_HELPER}" >&2
  exit 1
fi
exec python3 "\${PY_HELPER}" "$@"
EOF

chmod 755 "${WRAPPER_TMP}"
mv "${WRAPPER_TMP}" "${WRAPPER}"

echo
echo "Installation complete. To use this from the e2e tests, configure:"
echo
echo "  export TIGERVNC_WAN_HELPER=\"sudo -n ${WRAPPER}\""
echo
echo "and add an appropriate sudoers rule (for example):"
echo
echo "  youruser ALL=(root) NOPASSWD: ${WRAPPER} *" 

echo
echo "Review the security implications of this configuration before enabling it on a shared system."
