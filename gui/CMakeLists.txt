cmake_minimum_required(VERSION 3.21)
project(netusb-gui VERSION 0.2.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

find_package(Qt6 6.4 REQUIRED COMPONENTS Core Gui Network Qml Quick QuickControls2 Widgets Test)

qt_standard_project_setup()

add_library(netusb-core STATIC
    src/controllink.cpp
    src/controllink.h
    src/preferences.cpp
    src/preferences.h
    src/theme.cpp
    src/theme.h
    src/tray.cpp
    src/tray.h
)
target_include_directories(netusb-core PUBLIC src)
target_compile_definitions(netusb-core PUBLIC NETUSB_VERSION="${PROJECT_VERSION}")
target_link_libraries(netusb-core PUBLIC Qt6::Core Qt6::Gui Qt6::Network Qt6::Widgets)

qt_add_executable(netusb src/main.cpp)
if(WIN32)
    set_target_properties(netusb PROPERTIES WIN32_EXECUTABLE ON)
endif()

qt_add_qml_module(netusb
    URI Netusb
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
        icons/netusb.svg
)
target_link_libraries(netusb PRIVATE netusb-core Qt6::Qml Qt6::Quick Qt6::QuickControls2)

enable_testing()
qt_add_executable(tst_controllink tests/tst_controllink.cpp)
target_link_libraries(tst_controllink PRIVATE netusb-core Qt6::Test)
add_test(NAME controllink COMMAND tst_controllink)

include(GNUInstallDirs)
install(TARGETS netusb RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR})
if(UNIX AND NOT APPLE)
    install(FILES icons/netusb.svg DESTINATION ${CMAKE_INSTALL_DATADIR}/icons/hicolor/scalable/apps)
    install(FILES netusb.desktop DESTINATION ${CMAKE_INSTALL_DATADIR}/applications)
endif()
