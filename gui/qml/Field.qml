import QtQuick
import QtQuick.Controls

TextField {
    implicitHeight: 34
    color: theme.text
    placeholderTextColor: theme.textMuted
    selectionColor: theme.accent
    selectedTextColor: theme.accentText
    font.pixelSize: 14
    leftPadding: 10
    rightPadding: 10

    background: Rectangle {
        radius: 8
        color: theme.background
        border.color: parent.activeFocus ? theme.accent : theme.border
        border.width: parent.activeFocus ? 2 : 1
    }
}
