import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ApplicationWindow {
    id: window

    width: 920
    height: 700
    minimumWidth: 560
    minimumHeight: 440
    title: "Porthouse"
    color: theme.background

    onClosing: function (close) {
        if (trayAvailable) {
            close.accepted = false
            window.hide()
        } else {
            Qt.quit()
        }
    }

    function notify(message) {
        toastText.text = message
        toast.opacity = 1
        toastTimer.restart()
    }

    header: Rectangle {
        implicitHeight: 52
        color: theme.surface

        Rectangle {
            anchors.bottom: parent.bottom
            width: parent.width
            height: 1
            color: theme.border
        }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 20
            anchors.rightMargin: 20
            spacing: 24

            Text {
                text: "Porthouse"
                color: theme.text
                font.pixelSize: 17
                font.bold: true
            }

            Repeater {
                model: [qsTr("Use devices"), qsTr("Share devices"), qsTr("Activity")]

                delegate: Item {
                    id: tab

                    required property int index
                    required property string modelData
                    readonly property bool current: stack.currentIndex === index

                    Layout.fillHeight: true
                    implicitWidth: tabLabel.implicitWidth

                    Text {
                        id: tabLabel
                        anchors.centerIn: parent
                        text: tab.modelData
                        color: tab.current ? theme.accent : theme.textMuted
                        font.pixelSize: 14
                        font.bold: tab.current
                    }
                    Rectangle {
                        anchors.bottom: parent.bottom
                        width: parent.width
                        height: 2
                        color: theme.accent
                        visible: tab.current
                    }
                    MouseArea {
                        anchors.fill: parent
                        cursorShape: Qt.PointingHandCursor
                        onClicked: stack.currentIndex = tab.index
                    }
                }
            }

            Item {
                Layout.fillWidth: true
            }
        }
    }

    StackLayout {
        id: stack
        anchors.fill: parent
        currentIndex: firstTab === "share" ? 1 : firstTab === "activity" ? 2 : 0

        UseView {
            onMessage: text => window.notify(text)
        }
        ShareView {
            onMessage: text => window.notify(text)
        }
        ActivityView {}
    }

    Rectangle {
        id: toast
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 20
        width: Math.min(parent.width - 32, toastText.implicitWidth + 32)
        height: toastText.implicitHeight + 20
        radius: 10
        color: theme.text
        opacity: 0
        visible: opacity > 0

        Behavior on opacity {
            NumberAnimation {
                duration: 180
            }
        }

        Text {
            id: toastText
            anchors.centerIn: parent
            width: parent.width - 32
            color: theme.background
            font.pixelSize: 14
            wrapMode: Text.WordWrap
            horizontalAlignment: Text.AlignHCenter
        }

        Timer {
            id: toastTimer
            interval: 4000
            onTriggered: toast.opacity = 0
        }
    }
}
