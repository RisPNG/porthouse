cmake_minimum_required(VERSION 3.21)

include(cmake/version.cmake)
project(porthouse VERSION ${PORTHOUSE_VERSION} LANGUAGES CXX)

# Directory holding the release builds of porthouse-server, porthouse-clientd and porthouse-client.
set(PORTHOUSE_RUST_BIN_DIR "${CMAKE_SOURCE_DIR}/target/x86_64-unknown-linux-musl/release" CACHE PATH "Rust program directory")

add_subdirectory(gui)

include(GNUInstallDirs)
foreach(program porthouse-server porthouse-clientd porthouse-client)
    if(NOT EXISTS "${PORTHOUSE_RUST_BIN_DIR}/${program}")
        message(FATAL_ERROR "${PORTHOUSE_RUST_BIN_DIR}/${program} is missing; build the Rust programs first")
    endif()
    install(PROGRAMS "${PORTHOUSE_RUST_BIN_DIR}/${program}" DESTINATION ${CMAKE_INSTALL_BINDIR})
endforeach()

install(FILES packaging/linux/porthouse-server.service packaging/linux/porthouse-client.service
        DESTINATION lib/systemd/system)
install(FILES packaging/linux/porthouse-modules.conf
        DESTINATION lib/modules-load.d RENAME porthouse.conf)
install(FILES packaging/linux/porthouse.sysusers
        DESTINATION lib/sysusers.d RENAME porthouse.conf)
install(FILES packaging/linux/porthouse-autostart.desktop
        DESTINATION /etc/xdg/autostart RENAME porthouse.desktop)
install(FILES LICENSE THIRD-PARTY-LICENSES.md DESTINATION ${CMAKE_INSTALL_DOCDIR})
install(DIRECTORY licenses DESTINATION ${CMAKE_INSTALL_DOCDIR})

include(packaging/linux/cpack.cmake)
