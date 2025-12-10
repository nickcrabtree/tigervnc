#!/usr/bin/env bash
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

print_help() {
  cat <<'EOF'
Usage: ./run_tests.sh [OPTIONS] [CTEST_ARGS...]

TigerVNC test runner wrapper.

By default this runs:
  1) Rust tests under rust-vnc-viewer/ (via `cargo test`)
  2) All CTest tests in the configured build directory (including label "e2e")
  3) Standalone e2e Python/shell tests in tests/e2e/test_*.py and test_*.sh

Options:
  -h, --help          Show this help message and exit
  --build-dir DIR     Use an alternative CMake build directory
                      (default: build or value of $BUILD_DIR)

All remaining arguments are passed directly to ctest. This lets you use
standard CTest filters, for example:
  ./run_tests.sh -LE e2e        # Core tests only (exclude label "e2e")
  ./run_tests.sh -L e2e         # End-to-end tests only
  ./run_tests.sh -R some_test   # Tests matching name pattern

Environment variables:
  BUILD_DIR             Override default build directory (default: build)
  TIGERVNC_TEST_JOBS    Override number of parallel jobs for CTest
                         (defaults to nproc or 2 if unknown)

Examples:
  ./run_tests.sh
  ./run_tests.sh -LE e2e
  ./run_tests.sh --build-dir build-debug -R my_test

Notes:
  - You must configure and build the project first, for example:
        cmake -S . -B build
        cmake --build build
  - Rust tests and standalone e2e scripts may fail depending on your
    environment; their failures are reported with warnings and will
    cause run_tests.sh to exit with a non-zero status.
  - All test phases report their own warnings; the wrapper exits with
    a failing status code if any phase fails.
EOF
}

BUILD_DIR_DEFAULT="${BUILD_DIR:-build}"
BUILD_DIR="${BUILD_DIR_DEFAULT}"

QUIET=0
CTEST_ARGS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help)
      print_help
      exit 0
      ;;
    -q|--quiet)
      QUIET=1
      shift
      ;;
    --build-dir)
      shift
      if [[ $# -eq 0 ]]; then
        echo "ERROR: --build-dir requires an argument" >&2
        exit 2
      fi
      BUILD_DIR="$1"
      shift
      ;;
    --build-dir=*)
      BUILD_DIR="${1#*=}"
      shift
      ;;
    *)
      CTEST_ARGS+=("$1")
      shift
      ;;
  esac
done

if [[ -z "${BUILD_DIR}" ]]; then
  echo "ERROR: --build-dir cannot be empty" >&2
  exit 2
fi

if [[ ! -d "${BUILD_DIR}" ]]; then
  echo "ERROR: CMake build directory '${BUILD_DIR}' not found." >&2
  echo "       Configure and build the project first, for example:" >&2
  echo "         cmake -S . -B ${BUILD_DIR}" >&2
  echo "         cmake --build ${BUILD_DIR}" >&2
  exit 1
fi

if [[ -n "${TIGERVNC_TEST_JOBS-}" ]]; then
  JOBS="${TIGERVNC_TEST_JOBS}"
else
  if command -v nproc >/dev/null 2>&1; then
    JOBS="$(nproc)"
  elif command -v sysctl >/dev/null 2>&1; then
    JOBS="$(sysctl -n hw.ncpu 2>/dev/null || echo 2)"
  else
    JOBS="2"
  fi
fi

GLOBAL_STATUS=0

# Propagate quiet mode to sub-tools
if [[ ${QUIET} -eq 1 ]]; then
  export TIGERVNC_TEST_QUIET=1
else
  unset TIGERVNC_TEST_QUIET 2>/dev/null || true
fi

run_rust_tests() {
  if [[ ! -d "rust-vnc-viewer" ]]; then
    return
  fi
  if ! command -v cargo >/dev/null 2>&1; then
    echo "==> Skipping Rust tests (cargo not found)" >&2
    return
  fi

  if [[ ${QUIET} -eq 0 ]]; then
    echo "==> Running Rust tests (cargo test) in rust-vnc-viewer" >&2
  fi

  set +e
  (
    cd rust-vnc-viewer
    cargo test
  )
  local status=$?
  set -e

  if [[ ${status} -ne 0 ]]; then
    echo >&2
    echo "WARNING: Rust tests reported failures (exit code ${status})." >&2
    GLOBAL_STATUS=1
  fi
}

run_ctest_all() {
  if [[ ${QUIET} -eq 0 ]]; then
    echo "==> Running CTest in '${BUILD_DIR}' with -j${JOBS} (all tests, including e2e)" >&2
  fi

  # Ensure all unit-test gtest targets are built so that gtest_discover_tests
  # has registered them with CTest. This avoids accidentally skipping unit
  # tests when the main build was done with a narrower target (e.g. viewer
  # only).
  set +e
  local unit_targets=(
    configargs
    arccache
    bandwidthstats
    conv
    convertlf
    gesturehandler
    hostport
    parameters
    serverhashset
    persistentcache_protocol
    decode_manager
    pixelformat
    shortcuthandler
    unicode
    emulatemb
    tiling_analysis
  )
  cmake --build "${BUILD_DIR}" --target "${unit_targets[@]}" >/dev/null 2>&1
  local build_status=$?
  if [[ ${build_status} -ne 0 ]]; then
    echo >&2
    echo "WARNING: Failed to build one or more unit-test targets (exit ${build_status})." >&2
    GLOBAL_STATUS=1
  fi

  ctest --test-dir "${BUILD_DIR}" --output-on-failure -j"${JOBS}" "${CTEST_ARGS[@]}"
  local status=$?
  set -e

  if [[ ${status} -ne 0 ]]; then
    echo >&2
    echo "WARNING: CTest reported failures (exit code ${status})." >&2
    GLOBAL_STATUS=1
  fi
}

run_python_e2e_tests() {
  local e2e_dir="tests/e2e"

  if [[ ! -d "${e2e_dir}" ]]; then
    return
  fi

  if [[ ${QUIET} -eq 0 ]]; then
    echo "==> Running standalone e2e scripts in ${e2e_dir} (test_*.py, test_*.sh)" >&2
  fi

  if ! command -v python3 >/dev/null 2>&1; then
    echo "    Skipping Python e2e tests (python3 not found)" >&2
    return
  fi

  set +e
  local old_pwd
  old_pwd="$(pwd)"
  cd "${e2e_dir}" || exit 1

  # Best-effort global cleanup of any stray test VNC servers on the
  # dedicated test displays (:998/:999) before running the standalone
  # e2e scripts. This helps avoid "port 6898 already in use" failures
  # when a previous test run crashed or was interrupted.
  python3 - << 'PY'
from framework import best_effort_cleanup_test_server

# Pre-suite cleanup
best_effort_cleanup_test_server(998, 6898, verbose=True)
best_effort_cleanup_test_server(999, 6899, verbose=True)
PY

  for script in test_*; do
    if [[ ! -f "${script}" ]]; then
      continue
    fi
    case "${script}" in
      *.py)
        if [[ ${QUIET} -eq 0 ]]; then
          echo "    -> python3 ${script}" >&2
        fi
        python3 "${script}"
        local status=$?
        ;;
      *.sh)
        if [[ ${QUIET} -eq 0 ]]; then
          echo "    -> bash ${script}" >&2
        fi
        bash "${script}"
        local status=$?
        ;;
      *)
        continue
        ;;
    esac

    if [[ ${status} -ne 0 ]]; then
      echo >&2
      echo "WARNING: e2e script ${script} reported failures (exit code ${status})." >&2
      GLOBAL_STATUS=1
    fi

    # Post-script cleanup: ensure that a misbehaving or crashed test cannot
    # leave :998/:999 servers running and interfere with subsequent tests.
    python3 - << 'PY'
from framework import best_effort_cleanup_test_server
best_effort_cleanup_test_server(998, 6898, verbose=True)
best_effort_cleanup_test_server(999, 6899, verbose=True)
PY
  done
  cd "${old_pwd}" || true
  set -e
}

run_rust_tests
run_ctest_all
run_python_e2e_tests

exit "${GLOBAL_STATUS}"
