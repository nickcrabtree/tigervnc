# End-to-end tests for ContentCache validation

if(CMAKE_SOURCE_DIR STREQUAL CMAKE_CURRENT_SOURCE_DIR)
  message(FATAL_ERROR "cmake must be invoked with the top level directory")
endif()

# Option to enable end-to-end tests (disabled by default so CI must opt in)
option(TIGERVNC_E2E "Enable end-to-end ContentCache tests" OFF)

if(TIGERVNC_E2E AND Python3_FOUND)
  find_program(XTIGERVNC_EXECUTABLE Xtigervnc)

  if(XTIGERVNC_EXECUTABLE)
    # Original ContentCache parity test runner
    add_test(
      NAME e2e_contentcache
      COMMAND ${Python3_EXECUTABLE} ${CMAKE_CURRENT_SOURCE_DIR}/run_contentcache_test.py --verbose
    )

    set_tests_properties(e2e_contentcache PROPERTIES
      LABELS "e2e;contentcache"
      TIMEOUT 600  # 10 minute timeout
    )

    # Black-box screenshot-based visual corruption tests
    # 1) Sanity: both viewers with caches disabled (should be identical)
    add_test(
      NAME e2e_black_box_screenshot_none
      COMMAND ${Python3_EXECUTABLE} ${CMAKE_CURRENT_SOURCE_DIR}/run_black_box_screenshot_test.py --mode none
    )

    set_tests_properties(e2e_black_box_screenshot_none PROPERTIES
      LABELS "e2e;contentcache;visual;sanity"
      TIMEOUT 600  # 10 minute timeout
    )

    # 2) ContentCache enabled vs cache-off baseline
    add_test(
      NAME e2e_black_box_screenshot_content
      COMMAND ${Python3_EXECUTABLE} ${CMAKE_CURRENT_SOURCE_DIR}/run_black_box_screenshot_test.py --mode content
    )

    set_tests_properties(e2e_black_box_screenshot_content PROPERTIES
      LABELS "e2e;contentcache;visual;contentcache"
      TIMEOUT 600  # 10 minute timeout
    )

    # 3) PersistentCache enabled vs cache-off baseline
    add_test(
      NAME e2e_black_box_screenshot_persistent
      COMMAND ${Python3_EXECUTABLE} ${CMAKE_CURRENT_SOURCE_DIR}/run_black_box_screenshot_test.py --mode persistent
    )

    set_tests_properties(e2e_black_box_screenshot_persistent PROPERTIES
      LABELS "e2e;contentcache;visual;persistentcache"
      TIMEOUT 600  # 10 minute timeout
    )
  else()
    message(STATUS "Xtigervnc not found, e2e tests disabled")
  endif()
endif()
