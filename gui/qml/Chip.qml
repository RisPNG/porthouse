import QtQuick
import QtQuick.Layouts

Rectangle {
    property string text
    property color tone: theme.textMuted

    implicitWidth: label.implicitWidth + 18
    implicitHeight: label.implicitHeight + 6
    radius: height / 2
    color: theme.chip

    Text {
        id: label
        anchors.centerIn: parent
        text: parent.text
        color: parent.tone
        font.pixelSize: 12
    }
}
