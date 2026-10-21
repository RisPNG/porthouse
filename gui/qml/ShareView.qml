import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "Format.js" as Format

ScrollView {
    id: view

    signal message(string text)

    readonly property var state: serverLink.state
    readonly property var pairing: state.pairing !== undefined && state.pairing !== null ? state.pairing : null
    readonly property var clients: state.clients !== undefined ? state.clients : []
    readonly property var devices: state.devices !== undefined ? state.devices : []
    property int now: Math.floor(Date.now() / 1000)

    contentWidth: availableWidth
    clip: true

    Timer {
        interval: 1000
        running: view.pairing !== null && view.visible
        repeat: true
        onTriggered: view.now = Math.floor(Date.now() / 1000)
    }

    Connections {
        target: serverLink

        function onReplied(tag, ok, result) {
            if (!ok) {
                view.message(result)
            } else if (tag === "revoke") {
                view.message(qsTr("%1 can no longer use your devices").arg(result.name))
            }
            if (tag === "open_pairing" && ok) {
                view.now = Math.floor(Date.now() / 1000)
            }
        }
    }

    ColumnLayout {
        width: view.availableWidth
        spacing: 12

        Item {
            Layout.preferredHeight: 4
        }

        Banner {
            Layout.leftMargin: 20
            Layout.rightMargin: 20
            visible: !serverLink.connected
            title: qsTr("The netusb server service is not running")
            text: qsTr("It is what shares this computer’s devices. Start it, then this page fills in on its own. On Linux: sudo systemctl start netusb-server. On Windows: start the “netusb server” service.")
        }

        Card {
            Layout.leftMargin: 20
            Layout.rightMargin: 20
            visible: serverLink.connected

            ColumnLayout {
                Layout.fillWidth: true
                Layout.margins: 14
                spacing: 4

                Text {
                    text: view.state.name !== undefined ? view.state.name : ""
                    color: theme.text
                    font.pixelSize: 16
                    font.bold: true
                }
                Text {
                    text: qsTr("Sharing on port %1").arg(view.state.port !== undefined ? view.state.port : "")
                    color: theme.textMuted
                    font.pixelSize: 13
                }
                Text {
                    Layout.fillWidth: true
                    text: qsTr("Fingerprint %1").arg(view.state.fingerprint !== undefined ? view.state.fingerprint.substring(0, 32) : "")
                    color: theme.textMuted
                    font.pixelSize: 12
                    elide: Text.ElideRight
                }
            }
        }

        Text {
            visible: serverLink.connected
            Layout.leftMargin: 20
            text: qsTr("Pairing")
            color: theme.textMuted
            font.pixelSize: 13
            font.capitalization: Font.AllUppercase
            font.letterSpacing: 1
        }

        Card {
            Layout.leftMargin: 20
            Layout.rightMargin: 20
            visible: serverLink.connected

            ColumnLayout {
                Layout.fillWidth: true
                Layout.margins: 14
                spacing: 10

                Text {
                    Layout.fillWidth: true
                    visible: view.pairing === null
                    text: qsTr("Another computer has to be paired once before it can use your devices. Only pair computers you control: a paired computer can take any device you share, including your keyboard and mouse.")
                    color: theme.textMuted
                    font.pixelSize: 14
                    wrapMode: Text.WordWrap
                }
                Btn {
                    visible: view.pairing === null
                    primary: true
                    text: qsTr("Allow a new computer to pair")
                    onClicked: serverLink.call("open_pairing", {}, "open_pairing")
                }

                Text {
                    visible: view.pairing !== null
                    text: view.pairing !== null ? view.pairing.pin : ""
                    color: theme.text
                    font.pixelSize: 40
                    font.bold: true
                    font.letterSpacing: 4
                }
                Text {
                    Layout.fillWidth: true
                    visible: view.pairing !== null
                    text: view.pairing !== null ? qsTr("Enter this PIN on the other computer within %1. It works once.").arg(Format.countdown(view.pairing.expires, view.now)) : ""
                    color: theme.textMuted
                    font.pixelSize: 14
                    wrapMode: Text.WordWrap
                }
            }
        }

        Text {
            visible: serverLink.connected
            Layout.leftMargin: 20
            text: qsTr("Paired computers")
            color: theme.textMuted
            font.pixelSize: 13
            font.capitalization: Font.AllUppercase
            font.letterSpacing: 1
        }

        Card {
            Layout.leftMargin: 20
            Layout.rightMargin: 20
            visible: serverLink.connected

            Text {
                visible: view.clients.length === 0
                text: qsTr("No computer is paired yet.")
                color: theme.textMuted
                font.pixelSize: 14
                padding: 14
            }

            Repeater {
                model: view.clients

                delegate: ColumnLayout {
                    id: clientRow

                    required property var modelData
                    required property int index

                    Layout.fillWidth: true
                    spacing: 0

                    Rectangle {
                        visible: clientRow.index > 0
                        Layout.fillWidth: true
                        height: 1
                        color: theme.border
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        Layout.margins: 12
                        spacing: 12

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2
                            Text {
                                text: clientRow.modelData.name
                                color: theme.text
                                font.pixelSize: 14
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                            Text {
                                text: clientRow.modelData.fingerprint.substring(0, 16)
                                color: theme.textMuted
                                font.pixelSize: 12
                            }
                        }
                        Btn {
                            text: qsTr("Revoke")
                            onClicked: serverLink.call("revoke", {fingerprint: clientRow.modelData.fingerprint}, "revoke")
                        }
                    }
                }
            }
        }

        Text {
            visible: serverLink.connected
            Layout.leftMargin: 20
            text: qsTr("Devices on this computer")
            color: theme.textMuted
            font.pixelSize: 13
            font.capitalization: Font.AllUppercase
            font.letterSpacing: 1
        }

        Card {
            Layout.leftMargin: 20
            Layout.rightMargin: 20
            visible: serverLink.connected

            Repeater {
                model: view.devices

                delegate: ColumnLayout {
                    id: deviceRow

                    required property var modelData
                    required property int index

                    Layout.fillWidth: true
                    spacing: 0

                    Rectangle {
                        visible: deviceRow.index > 0
                        Layout.fillWidth: true
                        height: 1
                        color: theme.border
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        Layout.margins: 12
                        spacing: 10

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2
                            Text {
                                text: Format.deviceName(deviceRow.modelData)
                                color: theme.text
                                font.pixelSize: 14
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                            Text {
                                text: Format.deviceDetails(deviceRow.modelData)
                                color: theme.textMuted
                                font.pixelSize: 12
                            }
                        }
                        Chip {
                            visible: deviceRow.modelData.input
                            text: qsTr("input device")
                            tone: theme.warning
                        }
                        Chip {
                            visible: deviceRow.modelData.in_use_by !== null && deviceRow.modelData.in_use_by !== undefined
                            text: qsTr("in use by %1").arg(deviceRow.modelData.in_use_by)
                            tone: theme.good
                        }
                    }
                }
            }
        }

        Item {
            Layout.preferredHeight: 20
        }
    }
}
