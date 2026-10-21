# Reads the version from the workspace manifest so there is one place to change it.
file(STRINGS "${CMAKE_CURRENT_LIST_DIR}/../Cargo.toml" _netusb_version_line REGEX "^version = \"[0-9]+\\.[0-9]+\\.[0-9]+\"")
string(REGEX REPLACE "^version = \"([0-9.]+)\"" "\\1" NETUSB_VERSION "${_netusb_version_line}")
if(NOT NETUSB_VERSION)
    message(FATAL_ERROR "cannot read the version from Cargo.toml")
endif()
