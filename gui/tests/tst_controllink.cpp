#include <QJsonDocument>
#include <QJsonObject>
#include <QLocalServer>
#include <QLocalSocket>
#include <QSignalSpy>
#include <QTemporaryDir>
#include <QtTest>

#include "controllink.h"

class TestControlLink : public QObject {
    Q_OBJECT

private slots:
    void readsStateAndAnswersCalls();
    void reportsAMissingService();
};

void TestControlLink::readsStateAndAnswersCalls() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    qputenv("PORTHOUSE_IPC_DIR", dir.path().toLocal8Bit());

    QLocalServer server;
    QVERIFY(server.listen(dir.filePath(QStringLiteral("client.sock"))));
    QObject::connect(&server, &QLocalServer::newConnection, &server, [&server] {
        QLocalSocket *peer = server.nextPendingConnection();
        QObject::connect(peer, &QLocalSocket::readyRead, peer, [peer] {
            while (peer->canReadLine()) {
                const QJsonObject request = QJsonDocument::fromJson(peer->readLine()).object();
                const QString method = request.value(QStringLiteral("method")).toString();
                QJsonObject reply{{QStringLiteral("id"), request.value(QStringLiteral("id"))}};
                if (method == QLatin1String("state")) {
                    reply.insert(QStringLiteral("result"), QJsonObject{{QStringLiteral("version"), QStringLiteral("9.9")}});
                } else if (method == QLatin1String("echo")) {
                    reply.insert(QStringLiteral("result"), request.value(QStringLiteral("params")));
                } else {
                    reply.insert(QStringLiteral("error"), QStringLiteral("unknown method"));
                }
                peer->write(QJsonDocument(reply).toJson(QJsonDocument::Compact) + '\n');
            }
        });
    });

    ControlLink link(QStringLiteral("client"));
    QSignalSpy replies(&link, &ControlLink::replied);
    link.start(200);
    QTRY_VERIFY_WITH_TIMEOUT(link.connected(), 3000);
    QTRY_COMPARE_WITH_TIMEOUT(link.state().value(QStringLiteral("version")).toString(), QStringLiteral("9.9"), 3000);

    link.call(QStringLiteral("echo"), {{QStringLiteral("a"), 1}}, QStringLiteral("t1"));
    link.call(QStringLiteral("nope"), {}, QStringLiteral("t2"));
    QTRY_COMPARE_WITH_TIMEOUT(replies.count(), 2, 3000);
    QCOMPARE(replies.at(0).at(0).toString(), QStringLiteral("t1"));
    QVERIFY(replies.at(0).at(1).toBool());
    QCOMPARE(replies.at(0).at(2).toMap().value(QStringLiteral("a")).toInt(), 1);
    QCOMPARE(replies.at(1).at(0).toString(), QStringLiteral("t2"));
    QVERIFY(!replies.at(1).at(1).toBool());
    QCOMPARE(replies.at(1).at(2).toString(), QStringLiteral("unknown method"));
}

void TestControlLink::reportsAMissingService() {
    QTemporaryDir dir;
    qputenv("PORTHOUSE_IPC_DIR", dir.path().toLocal8Bit());
    ControlLink link(QStringLiteral("server"));
    QSignalSpy replies(&link, &ControlLink::replied);
    link.start(200);
    link.call(QStringLiteral("status"), {}, QStringLiteral("t"));
    QCOMPARE(replies.count(), 1);
    QVERIFY(!replies.at(0).at(1).toBool());
    QVERIFY(!link.connected());
}

QTEST_MAIN(TestControlLink)
#include "tst_controllink.moc"
