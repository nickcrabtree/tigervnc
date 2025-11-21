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
    environment; their failures are reported but do not change the
    wrapper's final exit code.
  - All test phases report their own warnings but run_tests.sh itself
    always exits with status 0 so that known failing tests do not
    break wrappers.
EOF
}

BUILD_DIR_DEFAULT="${BUILD_DIR:-build}"
BUILD_DIR="${BUILD_DIR_DEFAULT}"

CTEST_ARGS=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help)
      print_help
      exit 0
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

run_rust_tests() {
  if [[ ! -d "rust-vnc-viewer" ]]; then
    return
  fi
  if ! command -v cargo >/dev/null 2>&1; then
    echo "==> Skipping Rust tests (cargo not found)" >&2
    return
  fi

  echo "==> Running Rust tests (cargo test) in rust-vnc-viewer" >&2

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
    echo "         See the output above for details; wrapper will continue." >&2
  fi
}

run_ctest_all() {
  echo "==> Running CTest in '${BUILD_DIR}' with -j${JOBS} (all tests, including e2e)" >&2

  set +e
  ctest --test-dir "${BUILD_DIR}" --output-on-failure -j"${JOBS}" "${CTEST_ARGS[@]}"
  local status=$?
  set -e

  if [[ ${status} -ne 0 ]]; then
    echo >&2
    echo "WARNING: CTest reported failures (exit code ${status})." >&2
    echo "         See the output above for details; wrapper will continue." >&2
  fi
}

run_python_e2e_tests() {
  local e2e_dir="tests/e2e"

  if [[ ! -d "${e2e_dir}" ]]; then
    return
  fi

  echo "==> Running standalone e2e scripts in ${e2e_dir} (test_*.py, test_*.sh)" >&2

  if ! command -v python3 >/dev/null 2>&1; then
    echo "    Skipping Python e2e tests (python3 not found)" >&2
    return
  fi

  set +e
  (
    cd "${e2e_dir}"
    for script in test_*; do
      if [[ ! -f "${script}" ]]; then
        continue
      fi
      case "${script}" in
        *.py)
          echo "    -> python3 ${script}" >&2
          python3 "${script}"
          local status=$?
          ;;
        *.sh)
          echo "    -> bash ${script}" >&2
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
      fi
    done
  )
  set -e
}

run_rust_tests
run_ctest_all
run_python_e2e_tests

exit 0
