cmake_minimum_required(VERSION 3.21)
include(${CMAKE_CURRENT_LIST_DIR}/../cmake/version.cmake)
project(porthouse-gui VERSION ${PORTHOUSE_VERSION} LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 6.4 REQUIRED COMPONENTS Core Gui Network Qml Quick QuickControls2 Widgets Test)

qt_standard_project_setup()

add_library(porthouse-core STATIC
    src/controllink.cpp
    src/controllink.h
    src/preferences.cpp
    src/preferences.h
    src/theme.cpp
    src/theme.h
    src/tray.cpp
    src/tray.h
)
target_include_directories(porthouse-core PUBLIC src)
target_compile_definitions(porthouse-core PUBLIC PORTHOUSE_VERSION="${PROJECT_VERSION}")
target_link_libraries(porthouse-core PUBLIC Qt6::Core Qt6::Gui Qt6::Network Qt6::Widgets)

qt_add_executable(porthouse src/main.cpp)
if(WIN32)
    target_sources(porthouse PRIVATE porthouse.rc)
    set_target_properties(porthouse PROPERTIES WIN32_EXECUTABLE ON)
endif()

qt_add_qml_module(porthouse
    URI Porthouse
    VERSION 1.0
    QML_FILES
        qml/Main.qml
        qml/UseView.qml
        qml/ShareView.qml
        qml/ActivityView.qml
        qml/Banner.qml
        qml/Btn.qml
        qml/Card.qml
        qml/Chip.qml
        qml/Field.qml
        qml/Format.js
        qml/PairDialog.qml
        qml/InputDeviceDialog.qml
    RESOURCES
        icons/porthouse.svg
)
target_link_libraries(porthouse PRIVATE porthouse-core Qt6::Qml Qt6::Quick Qt6::QuickControls2)

enable_testing()
qt_add_executable(tst_controllink tests/tst_controllink.cpp)
target_link_libraries(tst_controllink PRIVATE porthouse-core Qt6::Test)
add_test(NAME controllink COMMAND tst_controllink)

include(GNUInstallDirs)
install(TARGETS porthouse RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR})
if(UNIX AND NOT APPLE)
    install(FILES icons/porthouse.svg DESTINATION ${CMAKE_INSTALL_DATADIR}/icons/hicolor/scalable/apps)
    install(FILES porthouse.desktop DESTINATION ${CMAKE_INSTALL_DATADIR}/applications)
endif()
