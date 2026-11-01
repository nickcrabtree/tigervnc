# C++ v2 cutover baseline — 2026-11-01

Revision: `fc51232225ad71526c3f57ea38a27c9e09a781f2`, master, clean before validation;
`git pull --ff-only` reported already up to date. Host: quartz, Linux x86_64,
kernel 7.0.0-38-generic; GCC C/C++ 15.2.0, CMake 4.2.3, cargo/rustc 1.96.0.
Existing `build/` configuration: Debug, `/usr/bin/cc`, `/usr/bin/c++`.
Runs took place at 19:57–19:59 GMT, outside the protected morning window.
Git, builds and tests ran with requested sandbox escalation; no production VNC was touched.

| Step | Result | Evidence / limit |
| --- | --- | --- |
| `cmake --build build` | FAIL, exit 2 | Missing `<string>` in `unix/tx/TXButton.h`; stopped without fixes |
| Rust workspace, all targets/features | PASS, exit 0 | 443 passed, 0 failed, 6 already ignored |
| `test_cpp_persistentcache.py` | BLOCKED, unrun | Runner selection limitation below |
| `test_rust_persistentcache.py` | BLOCKED, unrun | Runner selection limitation below |
| `test_cpp_cache_back_to_back.py` | BLOCKED, unrun | Runner selection limitation below |
| `test_cache_parity.py` | BLOCKED, unrun | Runner selection limitation below |
| `test_persistent_cache_bandwidth.py` | BLOCKED, unrun | Runner selection limitation below |
| `test_persistent_cache_eviction.py` | BLOCKED, unrun | Runner selection limitation below |
| C++ PersistentCache CTest selection | PASS, exit 0 | 30/30 existing binaries; not a successful HEAD rebuild |

Exact compiler errors while building `QueryConnectDialog.cxx`:

```text
unix/tx/TXButton.h:121:8: error: ‘string’ in namespace ‘std’ does not name a type
unix/tx/TXButton.h:59:5: error: ‘text’ was not declared in this scope; did you mean ‘text_’?
unix/tx/TXButton.h:81:36: error: ‘text’ was not declared in this scope
gmake[1]: *** [CMakeFiles/Makefile2:1977: unix/vncconfig/CMakeFiles/vncconfig.dir/all] Error 2
gmake: *** [Makefile:146: all] Error 2
```

The compiler also notes at `TXButton.h:32:1` that `std::string` is defined in `<string>`
and suggests adding `#include <string>`. The failing object is
`unix/vncconfig/CMakeFiles/vncconfig.dir/QueryConnectDialog.cxx.o`, rule line 121, error 1.
The compiler prints `/home/nickc/code/tigervnc/` paths, which resolve to this checkout.
No runtime test failed. Six Rust tests remain ignored by the existing suite, including
`persistent_cached_rect::tests::persistent_cached_rect_with_offset_blits_subrect`.
Cargo regenerated `Cargo.lock` during the requested unlocked invocation. The tested resolution
is preserved as `Cargo.tested.lock` in the evidence directory; the checkout's lockfile was restored
after the run. The Rust pass therefore applies to that resolved dependency set.

Commands used:

```bash
cmake --build build
cargo test --manifest-path rust-vnc-viewer/Cargo.toml --workspace --all-targets --all-features
ctest --test-dir build -R PersistentCache -LE e2e --output-on-failure --no-tests=error
```

CTest's discovered GoogleTest names use `PersistentCache`, hence the capitalized regex.
The four `persistentcache_*` unit executables have modification dates of 2026-07-27;
the failed build never reached their rebuild. Passing them is diagnostic evidence only.

`run_tests.sh` passes arguments only to CTest. Its standalone phase unconditionally loops
over every `tests/e2e/test_*` in glob order; it also performs global test-display cleanup.
The requested six scripts are not individually registered in `tests/e2e/CMakeLists.txt`.
Thus the requested selection/order cannot be expressed through the existing runner.
Running the full suite would exceed this validation scope; changing or bypassing the runner
would violate the validation-only/runner requirements. No e2e script was launched.

Gate 2's first checkbox remains unchecked because the C++ build failed and current-HEAD
C++ e2e acceptance is unestablished. No code or runner changes were made.

Full logs and reproducible execution script/README are durable under
`/ncloud/Nick/tigervnc-evidence/2026-11-01-cutover/`:

- `environment.txt`: revision, timestamp, host, compiler, CMake and Rust versions.
- `cpp-build.log`: complete build output and exit status.
- `rust-workspace.log`: complete workspace suite output and exit status.
- `cpp-persistentcache-ctest.log`: all 30 selected test names and results.
- `cpp-test-binary-times.txt`: existing C++ binary timestamps.
- `e2e-runner-inspection.log`: runner and CMake registration excerpts.
- `pre-commit.log`: staged-documentation check output (recorded after staging).
- `Cargo.tested.lock`: dependency resolution actually exercised by the Rust suite.

Staged-documentation pre-commit checks passed after correcting two Markdown line-length findings.
Only this document and the convergence checklist are staged; no commit or push was made.
