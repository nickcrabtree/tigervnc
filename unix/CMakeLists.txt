if(CMAKE_SOURCE_DIR STREQUAL CMAKE_CURRENT_SOURCE_DIR)
  message(FATAL_ERROR "cmake must be invoked with the top level directory")
endif()

add_subdirectory(tx)
add_subdirectory(common)
add_subdirectory(vncconfig)
add_subdirectory(vncpasswd)
add_subdirectory(vncserver)
add_subdirectory(x0vncserver)

if (ENABLE_WAYLAND)
  add_subdirectory(w0vncserver)
endif()

# Create symlink Xnjcvnc -> Xvnc for vncserver wrapper compatibility
add_custom_target(xnjcvnc_symlink ALL
  COMMAND ${CMAKE_COMMAND} -E create_symlink
    ${CMAKE_CURRENT_BINARY_DIR}/xserver/hw/vnc/Xnjcvnc
    ${CMAKE_CURRENT_BINARY_DIR}/vncserver/Xnjcvnc
  COMMENT "Creating Xnjcvnc symlink in vncserver directory for wrapper compatibility"
)
