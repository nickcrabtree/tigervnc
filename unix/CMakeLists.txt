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

# Create symlink Xtigervnc -> Xvnc for tigervncserver wrapper compatibility
add_custom_target(xtigervnc_symlink ALL
  COMMAND ${CMAKE_COMMAND} -E create_symlink
    ${CMAKE_CURRENT_BINARY_DIR}/xserver/xorg-server/hw/vnc/Xvnc
    ${CMAKE_CURRENT_BINARY_DIR}/vncserver/Xtigervnc
  COMMENT "Creating Xtigervnc symlink in vncserver directory for wrapper compatibility"
)
