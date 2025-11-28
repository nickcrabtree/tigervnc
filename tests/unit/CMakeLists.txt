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

add_executable(contentcache contentcache.cxx)
target_link_libraries(contentcache rfb core GTest::gtest_main)
gtest_discover_tests(contentcache)

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
