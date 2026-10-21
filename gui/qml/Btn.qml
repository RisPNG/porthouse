import QtQuick
import QtQuick.Controls

Button {
    id: control

    property bool primary: false

    implicitHeight: 34
    leftPadding: 14
    rightPadding: 14
    hoverEnabled: true

    contentItem: Text {
        text: control.text
        color: control.primary ? theme.accentText : theme.text
        font.pixelSize: 14
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        opacity: control.enabled ? 1 : 0.5
    }

    background: Rectangle {
        radius: 8
        color: control.primary ? theme.accent : theme.surface
        border.color: control.primary ? theme.accent : theme.border
        opacity: control.enabled ? (control.down ? 0.8 : control.hovered ? 0.92 : 1) : 0.5
        border.width: control.visualFocus ? 2 : 1
    }
}
