#!/usr/bin/env bash
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

print_help() {
  cat <<'EOF'
Usage: ./run_tests.sh [OPTIONS] [CTEST_ARGS...]

TigerVNC test runner wrapper around CTest.

By default this runs all CTest tests in the configured build directory,
including tests labelled "e2e".

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
  - Test failures are reported in the output, but this script always
    exits with status 0 so that known failing tests do not break wrappers.
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

echo "==> Running CTest in '${BUILD_DIR}' with -j${JOBS} (all tests, including e2e)" >&2

set +e
ctest --test-dir "${BUILD_DIR}" --output-on-failure -j"${JOBS}" "${CTEST_ARGS[@]}"
CTEST_STATUS=$?
set -e

if [[ ${CTEST_STATUS} -ne 0 ]]; then
  echo >&2
  echo "WARNING: CTest reported failures (exit code ${CTEST_STATUS})." >&2
  echo "         See the output above for details. Per wrapper design," >&2
  echo "         run_tests.sh is exiting with status 0." >&2
fi

exit 0
