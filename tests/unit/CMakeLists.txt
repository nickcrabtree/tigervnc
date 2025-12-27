include(GoogleTest)

# Google Test requires a new C++ standard
set(CMAKE_CXX_FLAGS "${CMAKE_CXX_FLAGS} -std=gnu++17")

enable_testing()

include_directories(${CMAKE_SOURCE_DIR}/common)
include_directories(${CMAKE_SOURCE_DIR}/vncviewer)

add_executable(configargs configargs.cxx)
target_link_libraries(configargs rfb GTest::gtest_main)
gtest_discover_tests(configargs)

add_executable(arccache arccache.cxx)
target_link_libraries(arccache rfb core GTest::gtest_main)
gtest_discover_tests(arccache)

add_executable(bandwidthstats bandwidthstats.cxx)
target_link_libraries(bandwidthstats rfb core GTest::gtest_main)
 gtest_discover_tests(bandwidthstats)
 
add_executable(conv conv.cxx)
target_link_libraries(conv rfb GTest::gtest_main)
gtest_discover_tests(conv)

add_executable(convertlf convertlf.cxx)
target_link_libraries(convertlf core GTest::gtest_main)
gtest_discover_tests(convertlf)

add_executable(gesturehandler gesturehandler.cxx ../../vncviewer/GestureHandler.cxx)
target_link_libraries(gesturehandler core GTest::gtest_main)
gtest_discover_tests(gesturehandler)

add_executable(hostport hostport.cxx)
target_link_libraries(hostport network GTest::gtest_main)
gtest_discover_tests(hostport)

add_executable(parameters parameters.cxx)
target_link_libraries(parameters core GTest::gtest_main)
gtest_discover_tests(parameters)

add_executable(serverhashset serverhashset.cxx)
target_link_libraries(serverhashset rfb core GTest::gtest_main)
gtest_discover_tests(serverhashset)

add_executable(persistentcache_protocol persistentcache_protocol.cxx)
target_link_libraries(persistentcache_protocol rfb core rdr network GTest::gtest_main)
gtest_discover_tests(persistentcache_protocol)

add_executable(global_client_persistent_cache_gc global_client_persistent_cache_gc.cxx)
target_link_libraries(global_client_persistent_cache_gc rfb core GTest::gtest_main)
gtest_discover_tests(global_client_persistent_cache_gc)

add_executable(persistentcache_pixelformat persistentcache_pixelformat.cxx)
target_link_libraries(persistentcache_pixelformat rfb core GTest::gtest_main)
gtest_discover_tests(persistentcache_pixelformat)

add_executable(persistentcache_crossbpp persistentcache_crossbpp.cxx)
target_link_libraries(persistentcache_crossbpp rfb core GTest::gtest_main)
gtest_discover_tests(persistentcache_crossbpp)

add_executable(persistentcache_quality persistentcache_quality.cxx)
target_link_libraries(persistentcache_quality rfb core GTest::gtest_main)
gtest_discover_tests(persistentcache_quality)

add_executable(test_hash_report_protocol test_hash_report_protocol.cxx)
target_link_libraries(test_hash_report_protocol rfb core rdr network GTest::gtest_main)
gtest_discover_tests(test_hash_report_protocol)

add_executable(decode_manager decode_manager.cxx)
target_link_libraries(decode_manager rfb core GTest::gtest_main)
target_compile_definitions(decode_manager PRIVATE UNIT_TEST)
gtest_discover_tests(decode_manager)

add_executable(pixelformat pixelformat.cxx)
target_link_libraries(pixelformat rfb GTest::gtest_main)
gtest_discover_tests(pixelformat)

add_executable(shortcuthandler shortcuthandler.cxx ../../vncviewer/ShortcutHandler.cxx)
target_link_libraries(shortcuthandler core ${Intl_LIBRARIES} GTest::gtest_main)
gtest_discover_tests(shortcuthandler)

add_executable(unicode unicode.cxx)
target_link_libraries(unicode core GTest::gtest_main)
gtest_discover_tests(unicode)

add_executable(emulatemb emulatemb.cxx ../../vncviewer/EmulateMB.cxx)
target_include_directories(emulatemb SYSTEM PUBLIC ${Intl_INCLUDE_DIR})
target_link_libraries(emulatemb core ${Intl_LIBRARIES} GTest::gtest_main)
gtest_discover_tests(emulatemb)

add_executable(tiling_analysis tiling_analysis.cxx)
target_link_libraries(tiling_analysis rfb core GTest::gtest_main)
gtest_discover_tests(tiling_analysis)

add_executable(cachecoordinator cachecoordinator.cxx)
target_link_libraries(cachecoordinator rfb core GTest::gtest_main)
gtest_discover_tests(cachecoordinator)

# Standalone test for lossy cache hash behavior (not using GTest)
add_executable(test_lossy_cache test_lossy_cache.cxx)
target_link_libraries(test_lossy_cache rfb core rdr)

# Standalone test for server-viewer lossy hash mapping flow (not using GTest)
add_executable(test_lossy_mapping test_lossy_mapping.cxx)
target_link_libraries(test_lossy_mapping rfb core rdr)
