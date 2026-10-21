import QtQuick
import QtQuick.Layouts

Rectangle {
    default property alias content: column.data

    Layout.fillWidth: true
    implicitHeight: column.implicitHeight
    radius: 12
    color: theme.surface
    border.color: theme.border
    border.width: 1
    clip: true

    ColumnLayout {
        id: column
        anchors.left: parent.left
        anchors.right: parent.right
        spacing: 0
    }
}
