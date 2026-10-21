#pragma once

#include <QIcon>
#include <QObject>
#include <QSystemTrayIcon>

class ControlLink;
class QAction;
class QMenu;
class Theme;

// The notification-area icon: shows whether anything is in use and offers the program's main actions.
class Tray : public QObject {
    Q_OBJECT

public:
    Tray(Theme *theme, ControlLink *client, ControlLink *server, QObject *parent = nullptr);

    static bool available();
    static QIcon icon(const Theme *theme, bool active);

signals:
    void openRequested();
    void quitRequested();

private:
    void update();

    Theme *m_theme;
    ControlLink *m_client;
    ControlLink *m_server;
    QSystemTrayIcon m_icon;
    QMenu *m_menu;
    QAction *m_usingLine;
    QAction *m_sharingLine;
};
