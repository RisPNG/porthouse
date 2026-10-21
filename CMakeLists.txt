cmake_minimum_required(VERSION 3.21)

include(cmake/version.cmake)
project(netusb VERSION ${NETUSB_VERSION} LANGUAGES CXX)

# Directory holding the release builds of netusb-server, netusb-clientd and netusb-client.
set(NETUSB_RUST_BIN_DIR "${CMAKE_SOURCE_DIR}/target/x86_64-unknown-linux-musl/release" CACHE PATH "Rust program directory")

add_subdirectory(gui)

include(GNUInstallDirs)
foreach(program netusb-server netusb-clientd netusb-client)
    if(NOT EXISTS "${NETUSB_RUST_BIN_DIR}/${program}")
        message(FATAL_ERROR "${NETUSB_RUST_BIN_DIR}/${program} is missing; build the Rust programs first")
    endif()
    install(PROGRAMS "${NETUSB_RUST_BIN_DIR}/${program}" DESTINATION ${CMAKE_INSTALL_BINDIR})
endforeach()

install(FILES packaging/linux/netusb-server.service packaging/linux/netusb-client.service
        DESTINATION lib/systemd/system)
install(FILES packaging/linux/netusb-modules.conf
        DESTINATION lib/modules-load.d RENAME netusb.conf)
install(FILES packaging/linux/netusb-autostart.desktop
        DESTINATION /etc/xdg/autostart RENAME netusb.desktop)
install(FILES LICENSE-MIT LICENSE-APACHE DESTINATION ${CMAKE_INSTALL_DOCDIR})

include(packaging/linux/cpack.cmake)
