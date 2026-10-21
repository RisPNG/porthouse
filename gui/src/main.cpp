#include <QApplication>
#include <QCommandLineParser>
#include <QDir>
#include <QLocalServer>
#include <QLocalSocket>
#include <QQmlApplicationEngine>
#include <QQmlContext>
#include <QQuickStyle>
#include <QQuickWindow>
#include <QTimer>

#include "controllink.h"
#include "preferences.h"
#include "theme.h"
#include "tray.h"

namespace {

QString instanceName() {
    return QStringLiteral("porthouse-gui-") + QString::fromLocal8Bit(qgetenv("USER").isEmpty() ? qgetenv("USERNAME") : qgetenv("USER"));
}

void showWindow(QQuickWindow *window) {
    if (!window) {
        return;
    }
    window->show();
    window->raise();
    window->requestActivate();
}

}  // namespace

int main(int argc, char *argv[]) {
    QApplication app(argc, argv);
    QApplication::setApplicationName(QStringLiteral("porthouse"));
    QApplication::setOrganizationName(QStringLiteral("porthouse"));
    QApplication::setApplicationVersion(QStringLiteral(PORTHOUSE_VERSION));
    QApplication::setQuitOnLastWindowClosed(false);
    QQuickStyle::setStyle(QStringLiteral("Basic"));

    QCommandLineParser parser;
    parser.setApplicationDescription(QStringLiteral("Use USB devices that are plugged into other computers, and share yours."));
    parser.addHelpOption();
    parser.addVersionOption();
    QCommandLineOption minimized(QStringLiteral("minimized"), QStringLiteral("Start in the notification area without opening the window."));
    QCommandLineOption screenshot(QStringLiteral("screenshot"), QStringLiteral("Save a picture of the window to FILE and exit."), QStringLiteral("FILE"));
    QCommandLineOption tab(QStringLiteral("tab"), QStringLiteral("Open this tab first: use, share or activity."), QStringLiteral("NAME"), QStringLiteral("use"));
    parser.addOption(minimized);
    parser.addOption(screenshot);
    parser.addOption(tab);
    parser.process(app);

    // Only one copy runs per person; a second start brings the first to the front.
    if (!parser.isSet(screenshot)) {
        QLocalSocket probe;
        probe.connectToServer(instanceName());
        if (probe.waitForConnected(300)) {
            probe.write("show\n");
            probe.waitForBytesWritten(300);
            return 0;
        }
    }
    QLocalServer instance;
    QLocalServer::removeServer(instanceName());
    if (!parser.isSet(screenshot)) {
        instance.listen(instanceName());
    }

    Theme theme;
    Preferences prefs;
    ControlLink clientLink(QStringLiteral("client"));
    ControlLink serverLink(QStringLiteral("server"));
    clientLink.start();
    serverLink.start();

    const bool trayAvailable = Tray::available() && !parser.isSet(screenshot);
    Tray *tray = nullptr;
    if (trayAvailable) {
        tray = new Tray(&theme, &clientLink, &serverLink, &app);
    }
    QApplication::setWindowIcon(Tray::icon(&theme, false));

    QQmlApplicationEngine engine;
    engine.rootContext()->setContextProperty(QStringLiteral("theme"), &theme);
    engine.rootContext()->setContextProperty(QStringLiteral("prefs"), &prefs);
    engine.rootContext()->setContextProperty(QStringLiteral("clientLink"), &clientLink);
    engine.rootContext()->setContextProperty(QStringLiteral("serverLink"), &serverLink);
    engine.rootContext()->setContextProperty(QStringLiteral("trayAvailable"), trayAvailable);
    engine.rootContext()->setContextProperty(QStringLiteral("firstTab"), parser.value(tab));
    engine.load(QUrl(QStringLiteral("qrc:/Porthouse/qml/Main.qml")));
    if (engine.rootObjects().isEmpty()) {
        return 1;
    }
    auto *window = qobject_cast<QQuickWindow *>(engine.rootObjects().first());

    if (!trayAvailable || !parser.isSet(minimized)) {
        showWindow(window);
    }
    if (tray) {
        QObject::connect(tray, &Tray::openRequested, &app, [window] { showWindow(window); });
        QObject::connect(tray, &Tray::quitRequested, &app, &QApplication::quit);
    }
    QObject::connect(&instance, &QLocalServer::newConnection, &app, [&instance, window] {
        while (QLocalSocket *peer = instance.nextPendingConnection()) {
            peer->deleteLater();
        }
        showWindow(window);
    });

    if (parser.isSet(screenshot)) {
        const QString path = parser.value(screenshot);
        QTimer::singleShot(2500, &app, [&app, window, path] {
            const bool saved = window->grabWindow().save(path);
            app.exit(saved ? 0 : 2);
        });
    }
    return app.exec();
}
