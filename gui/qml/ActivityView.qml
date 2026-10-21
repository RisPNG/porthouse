import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "Format.js" as Format

ScrollView {
    id: view

    readonly property var lines: clientLink.state.events !== undefined ? Format.eventLines(clientLink.state.events) : []

    contentWidth: availableWidth
    clip: true

    ColumnLayout {
        width: view.availableWidth
        spacing: 12

        Item {
            Layout.preferredHeight: 4
        }

        Card {
            Layout.leftMargin: 20
            Layout.rightMargin: 20

            Text {
                Layout.fillWidth: true
                text: view.lines.length > 0 ? view.lines.join("\n") : qsTr("Nothing has happened yet.")
                color: theme.textMuted
                font.pixelSize: 13
                font.family: "monospace"
                wrapMode: Text.WrapAnywhere
                padding: 14
            }
        }

        Item {
            Layout.preferredHeight: 20
        }
    }
}
