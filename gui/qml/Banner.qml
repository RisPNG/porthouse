import QtQuick
import QtQuick.Layouts

Rectangle {
    id: root

    property string title
    property string text
    property color tone: theme.warning

    Layout.fillWidth: true
    implicitHeight: body.implicitHeight + 24
    radius: 10
    color: theme.surface
    border.color: tone
    border.width: 1

    ColumnLayout {
        id: body
        anchors.fill: parent
        anchors.margins: 12
        spacing: 2

        Text {
            text: title
            color: tone
            font.pixelSize: 14
            font.bold: true
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
        }
        Text {
            text: root.text
            color: theme.text
            font.pixelSize: 13
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
        }
    }
}
