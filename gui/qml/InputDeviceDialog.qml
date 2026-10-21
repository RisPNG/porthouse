import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: dialog

    property string deviceName
    property string serverName
    signal confirmed

    modal: true
    anchors.centerIn: parent
    width: Math.min(parent.width - 32, 460)
    padding: 20

    function openFor(device, server) {
        deviceName = device
        serverName = server
        remember.checked = false
        open()
    }

    Overlay.modal: Rectangle {
        color: theme.overlay
    }

    background: Rectangle {
        radius: 12
        color: theme.surface
        border.color: theme.border
    }

    header: Text {
        text: qsTr("Use this input device here?")
        color: theme.text
        font.pixelSize: 17
        font.bold: true
        padding: 20
        bottomPadding: 0
    }

    contentItem: ColumnLayout {
        spacing: 12

        Text {
            Layout.fillWidth: true
            text: qsTr("“%1” is a mouse, keyboard or similar device. While you use it here, %2 does not have it. Release it here, or unplug and replug it, to give it back.").arg(dialog.deviceName).arg(dialog.serverName)
            color: theme.text
            font.pixelSize: 13
            wrapMode: Text.WordWrap
        }
        CheckBox {
            id: remember
            text: qsTr("Do not ask again")
            contentItem: Text {
                text: remember.text
                color: theme.text
                font.pixelSize: 13
                leftPadding: remember.indicator.width + 8
                verticalAlignment: Text.AlignVCenter
            }
        }
        RowLayout {
            Layout.alignment: Qt.AlignRight
            spacing: 8
            Btn {
                text: qsTr("Cancel")
                onClicked: dialog.close()
            }
            Btn {
                primary: true
                text: qsTr("Use it here")
                onClicked: {
                    if (remember.checked) {
                        prefs.confirmInputDevices = false
                    }
                    dialog.close()
                    dialog.confirmed()
                }
            }
        }
    }
}
