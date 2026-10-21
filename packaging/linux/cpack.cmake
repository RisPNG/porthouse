set(CPACK_PACKAGE_NAME "netusb")
set(CPACK_PACKAGE_VENDOR "netusb")
set(CPACK_PACKAGE_VERSION "${NETUSB_VERSION}")
set(CPACK_PACKAGE_CONTACT "netusb <hello@rispeng.com>")
set(CPACK_PACKAGE_DESCRIPTION_SUMMARY "Use USB devices that are plugged into other computers")
set(CPACK_PACKAGE_DESCRIPTION "netusb shares USB devices over the network and uses devices shared by other computers. It has a server, a client service, a command line tool, and a program with a notification-area icon.")
set(CPACK_PACKAGE_HOMEPAGE_URL "https://github.com/RisPNG/netusb")
set(CPACK_RESOURCE_FILE_LICENSE "${CMAKE_SOURCE_DIR}/LICENSE-MIT")
set(CPACK_PACKAGING_INSTALL_PREFIX "/usr")
set(CPACK_SET_DESTDIR ON)
set(CPACK_STRIP_FILES ON)

# NETUSB_PACKAGE_FLAVOR names the system the package is built on, for example debian-12.
set(NETUSB_PACKAGE_FLAVOR "linux" CACHE STRING "System the package is built for")
set(CPACK_PACKAGE_FILE_NAME "netusb-${NETUSB_PACKAGE_FLAVOR}-${NETUSB_VERSION}-${CMAKE_SYSTEM_PROCESSOR}")

set(CPACK_DEBIAN_PACKAGE_SHLIBDEPS ON)
set(CPACK_DEBIAN_PACKAGE_SECTION "net")
set(CPACK_DEBIAN_PACKAGE_ARCHITECTURE "amd64")
set(CPACK_DEBIAN_FILE_NAME "netusb-${NETUSB_PACKAGE_FLAVOR}-${NETUSB_VERSION}-amd64.deb")
set(CPACK_DEBIAN_PACKAGE_DEPENDS "${NETUSB_DEB_DEPENDS}")
set(CPACK_DEBIAN_PACKAGE_CONTROL_EXTRA
    "${CMAKE_SOURCE_DIR}/packaging/linux/deb/postinst;${CMAKE_SOURCE_DIR}/packaging/linux/deb/prerm;${CMAKE_SOURCE_DIR}/packaging/linux/deb/postrm")
set(CPACK_DEBIAN_PACKAGE_CONTROL_STRICT_PERMISSION ON)

set(CPACK_RPM_PACKAGE_LICENSE "MIT OR Apache-2.0")
set(CPACK_RPM_PACKAGE_GROUP "Applications/System")
set(CPACK_RPM_PACKAGE_ARCHITECTURE "x86_64")
set(CPACK_RPM_FILE_NAME "netusb-${NETUSB_PACKAGE_FLAVOR}-${NETUSB_VERSION}-x86_64.rpm")
set(CPACK_RPM_PACKAGE_REQUIRES "${NETUSB_RPM_REQUIRES}")
set(CPACK_RPM_PACKAGE_AUTOREQ ON)
set(CPACK_RPM_POST_INSTALL_SCRIPT_FILE "${CMAKE_SOURCE_DIR}/packaging/linux/rpm/post.sh")
set(CPACK_RPM_PRE_UNINSTALL_SCRIPT_FILE "${CMAKE_SOURCE_DIR}/packaging/linux/rpm/preun.sh")
set(CPACK_RPM_POST_UNINSTALL_SCRIPT_FILE "${CMAKE_SOURCE_DIR}/packaging/linux/rpm/postun.sh")
set(CPACK_RPM_EXCLUDE_FROM_AUTO_FILELIST_ADDITION
    /usr/lib /usr/lib/systemd /usr/lib/systemd/system /usr/lib/modules-load.d /usr/lib/sysusers.d /etc /etc/xdg /etc/xdg/autostart
    /usr/share/applications /usr/share/icons /usr/share/icons/hicolor /usr/share/icons/hicolor/scalable
    /usr/share/icons/hicolor/scalable/apps /usr/share/doc)

include(CPack)
