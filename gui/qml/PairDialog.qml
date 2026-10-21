import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

Dialog {
    id: dialog

    property string serverName
    property string error
    property bool busy: false
    signal pinEntered(string pin)

    title: qsTr("Pair with %1").arg(serverName)
    modal: true
    anchors.centerIn: parent
    width: Math.min(parent.width - 32, 420)
    padding: 20

    function openFor(name) {
        serverName = name
        error = ""
        busy = false
        pin.text = ""
        open()
        pin.forceActiveFocus()
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
        text: dialog.title
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
            text: qsTr("On that computer, open Porthouse and press “Allow a new computer to pair”, or run “porthouse-server pair”. Enter the PIN it shows.")
            color: theme.textMuted
            font.pixelSize: 13
            wrapMode: Text.WordWrap
        }
        Field {
            id: pin
            Layout.fillWidth: true
            placeholderText: qsTr("PIN, for example 1234-5678")
            inputMethodHints: Qt.ImhDigitsOnly | Qt.ImhNoPredictiveText
            maximumLength: 16
            onAccepted: submit.clicked()
        }
        Text {
            visible: dialog.error.length > 0
            Layout.fillWidth: true
            text: dialog.error
            color: theme.bad
            font.pixelSize: 13
            wrapMode: Text.WordWrap
        }
        RowLayout {
            Layout.alignment: Qt.AlignRight
            spacing: 8
            Btn {
                text: qsTr("Cancel")
                onClicked: dialog.close()
            }
            Btn {
                id: submit
                primary: true
                text: dialog.busy ? qsTr("Pairing…") : qsTr("Pair")
                enabled: !dialog.busy && pin.text.length > 0
                onClicked: {
                    dialog.busy = true
                    dialog.error = ""
                    dialog.pinEntered(pin.text)
                }
            }
        }
    }
}
