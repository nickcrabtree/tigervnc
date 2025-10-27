# End-to-end tests for ContentCache validation

if(CMAKE_SOURCE_DIR STREQUAL CMAKE_CURRENT_SOURCE_DIR)
  message(FATAL_ERROR "cmake must be invoked with the top level directory")
endif()

# TODO: Add option for enabling e2e tests
# option(TIGERVNC_E2E "Enable end-to-end ContentCache tests" OFF)

# TODO: Add test with Python3 and Xtigervnc checks
# if(TIGERVNC_E2E AND Python3_FOUND)
#   find_program(XTIGERVNC_EXECUTABLE Xtigervnc)
#   
#   if(XTIGERVNC_EXECUTABLE)
#     add_test(
#       NAME e2e_contentcache
#       COMMAND ${Python3_EXECUTABLE} ${CMAKE_CURRENT_SOURCE_DIR}/run_contentcache_test.py --verbose
#     )
#     
#     set_tests_properties(e2e_contentcache PROPERTIES
#       LABELS "e2e;contentcache"
#       TIMEOUT 600  # 10 minute timeout
#     )
#   else()
#     message(STATUS "Xtigervnc not found, e2e tests disabled")
#   endif()
# endif()
