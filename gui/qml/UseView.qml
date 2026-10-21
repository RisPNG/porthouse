import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "Format.js" as Format

ScrollView {
    id: view

    signal message(string text)

    readonly property var state: clientLink.state
    readonly property var attachments: state.attachments !== undefined ? state.attachments : []
    readonly property var servers: state.servers !== undefined ? state.servers : []
    property var busy: ({})

    function setBusy(key, on) {
        var copy = Object.assign({}, busy)
        if (on) {
            copy[key] = true
        } else {
            delete copy[key]
        }
        busy = copy
    }

    function isAttached(addr, busid) {
        for (var i = 0; i < attachments.length; i++) {
            if (attachments[i].id === addr + "|" + busid) {
                return true
            }
        }
        return false
    }

    function useDevice(server, device) {
        var go = function () {
            setBusy(server.addr + "|" + device.busid, true)
            clientLink.call("use", {addr: server.addr, busid: device.busid}, "use:" + server.addr + "|" + device.busid)
        }
        if (device.input && prefs.confirmInputDevices) {
            inputDialog.confirmed.connect(function once() {
                inputDialog.confirmed.disconnect(once)
                go()
            })
            inputDialog.openFor(Format.deviceName(device), server.name)
        } else {
            go()
        }
    }

    contentWidth: availableWidth
    clip: true

    Connections {
        target: clientLink

        function onReplied(tag, ok, result) {
            var parts = tag.split(":")
            var kind = parts[0]
            var key = tag.substring(kind.length + 1)
            if (kind === "use" || kind === "release") {
                view.setBusy(key, false)
                if (!ok) {
                    view.message(result)
                }
            } else if (kind === "pair") {
                if (ok) {
                    pairDialog.close()
                    view.message(qsTr("Paired with %1").arg(result.name))
                } else {
                    pairDialog.busy = false
                    pairDialog.error = result
                }
            } else if (kind === "add" && !ok) {
                view.message(result)
            }
        }
    }

    PairDialog {
        id: pairDialog
        property string addr
        onPinEntered: pin => clientLink.call("pair", {addr: addr, pin: pin}, "pair:" + addr)
    }

    InputDeviceDialog {
        id: inputDialog
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
            visible: !clientLink.connected
            title: qsTr("The Porthouse client service is not running")
            text: qsTr("It lets this program attach devices. Start it, then this page fills in on its own. On Linux: sudo systemctl start porthouse-client. On Windows: start the “porthouse client” service.")
        }

        Text {
            Layout.leftMargin: 20
            text: qsTr("In use on this computer")
            color: theme.textMuted
            font.pixelSize: 13
            font.capitalization: Font.AllUppercase
            font.letterSpacing: 1
        }

        Card {
            Layout.leftMargin: 20
            Layout.rightMargin: 20

            Text {
                visible: view.attachments.length === 0
                text: qsTr("No devices in use. Pick one below.")
                color: theme.textMuted
                font.pixelSize: 14
                padding: 14
            }

            Repeater {
                model: view.attachments

                delegate: ColumnLayout {
                    id: attached

                    required property var modelData
                    required property int index

                    Layout.fillWidth: true
                    spacing: 0

                    Rectangle {
                        visible: attached.index > 0
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
                                text: qsTr("%1:%2 from %3").arg(Format.hex4(attached.modelData.vendor_id)).arg(Format.hex4(attached.modelData.product_id)).arg(attached.modelData.server_name)
                                color: theme.text
                                font.pixelSize: 14
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                            Text {
                                text: qsTr("%1  ·  virtual port %2").arg(attached.modelData.busid).arg(attached.modelData.port)
                                color: theme.textMuted
                                font.pixelSize: 12
                            }
                        }
                        Btn {
                            text: view.busy[attached.modelData.id] ? qsTr("Releasing…") : qsTr("Release")
                            enabled: !view.busy[attached.modelData.id]
                            onClicked: {
                                view.setBusy(attached.modelData.id, true)
                                clientLink.call("release", {id: attached.modelData.id}, "release:" + attached.modelData.id)
                            }
                        }
                    }
                }
            }
        }

        Text {
            Layout.leftMargin: 20
            text: qsTr("Servers")
            color: theme.textMuted
            font.pixelSize: 13
            font.capitalization: Font.AllUppercase
            font.letterSpacing: 1
        }

        Text {
            visible: view.servers.length === 0 && clientLink.connected
            Layout.leftMargin: 20
            Layout.rightMargin: 20
            Layout.fillWidth: true
            text: qsTr("Looking for computers that share devices… You can also add one by its address below.")
            color: theme.textMuted
            font.pixelSize: 14
            wrapMode: Text.WordWrap
        }

        Repeater {
            model: view.servers

            delegate: Card {
                id: serverCard

                required property var modelData

                Layout.leftMargin: 20
                Layout.rightMargin: 20

                RowLayout {
                    Layout.fillWidth: true
                    Layout.margins: 12
                    spacing: 10

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2
                        Text {
                            text: serverCard.modelData.name
                            color: theme.text
                            font.pixelSize: 15
                            font.bold: true
                            elide: Text.ElideRight
                            Layout.fillWidth: true
                        }
                        Text {
                            text: serverCard.modelData.addr
                            color: theme.textMuted
                            font.pixelSize: 12
                        }
                    }
                    Chip {
                        text: serverCard.modelData.error ? qsTr("unreachable") : serverCard.modelData.paired ? qsTr("paired") : qsTr("not paired")
                        tone: serverCard.modelData.error ? theme.bad : serverCard.modelData.paired ? theme.good : theme.warning
                    }
                    Btn {
                        visible: !serverCard.modelData.paired && !serverCard.modelData.error
                        primary: true
                        text: qsTr("Pair…")
                        onClicked: {
                            pairDialog.addr = serverCard.modelData.addr
                            pairDialog.openFor(serverCard.modelData.name)
                        }
                    }
                }

                Text {
                    visible: serverCard.modelData.error !== null && serverCard.modelData.error !== undefined
                    Layout.fillWidth: true
                    Layout.leftMargin: 12
                    Layout.rightMargin: 12
                    Layout.bottomMargin: 12
                    text: serverCard.modelData.error ? serverCard.modelData.error : ""
                    color: theme.bad
                    font.pixelSize: 13
                    wrapMode: Text.WordWrap
                }

                Text {
                    visible: serverCard.modelData.paired && serverCard.modelData.devices.length === 0
                    text: qsTr("This computer shares no devices.")
                    color: theme.textMuted
                    font.pixelSize: 14
                    padding: 12
                }

                Repeater {
                    model: serverCard.modelData.paired ? serverCard.modelData.devices : []

                    delegate: ColumnLayout {
                        id: deviceRow

                        required property var modelData
                        readonly property string key: serverCard.modelData.addr + "|" + modelData.busid
                        readonly property bool mine: view.isAttached(serverCard.modelData.addr, modelData.busid)
                        readonly property bool other: modelData.in_use_by !== null && modelData.in_use_by !== undefined && !mine

                        Layout.fillWidth: true
                        spacing: 0

                        Rectangle {
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
                                visible: deviceRow.mine
                                text: qsTr("in use here")
                                tone: theme.good
                            }
                            Chip {
                                visible: deviceRow.other
                                text: qsTr("in use by %1").arg(deviceRow.modelData.in_use_by)
                            }
                            Btn {
                                visible: !deviceRow.mine && !deviceRow.other
                                primary: true
                                text: view.busy[deviceRow.key] ? qsTr("Connecting…") : qsTr("Use")
                                enabled: !view.busy[deviceRow.key]
                                onClicked: view.useDevice(serverCard.modelData, deviceRow.modelData)
                            }
                        }
                    }
                }
            }
        }

        Card {
            Layout.leftMargin: 20
            Layout.rightMargin: 20

            RowLayout {
                Layout.fillWidth: true
                Layout.margins: 12
                spacing: 8

                Field {
                    id: addAddress
                    Layout.fillWidth: true
                    placeholderText: qsTr("Add a computer by address, for example 192.168.1.20")
                    onAccepted: addButton.clicked()
                }
                Btn {
                    id: addButton
                    primary: true
                    text: qsTr("Add")
                    enabled: addAddress.text.trim().length > 0 && clientLink.connected
                    onClicked: {
                        clientLink.call("add_server", {addr: addAddress.text.trim()}, "add:" + addAddress.text)
                        addAddress.text = ""
                    }
                }
            }
        }

        Item {
            Layout.preferredHeight: 20
        }
    }
}
