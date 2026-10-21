#pragma once

#include <QHash>
#include <QLocalSocket>
#include <QObject>
#include <QTimer>
#include <QVariantMap>

// One connection to a netusb service (client or server) over its local control channel.
// Requests and replies are one JSON object per line; the state of the service is polled.
class ControlLink : public QObject {
    Q_OBJECT
    Q_PROPERTY(QString service READ service CONSTANT)
    Q_PROPERTY(bool connected READ connected NOTIFY connectedChanged)
    Q_PROPERTY(QVariantMap state READ state NOTIFY stateChanged)

public:
    // `service` is "client" or "server".
    explicit ControlLink(const QString &service, QObject *parent = nullptr);
    ~ControlLink() override;

    QString service() const { return m_service; }
    bool connected() const { return m_connected; }
    QVariantMap state() const { return m_state; }

    void start(int pollMs = 2000);

    // The reply arrives through `replied` with the same tag.
    Q_INVOKABLE void call(const QString &method, const QVariantMap &params = {}, const QString &tag = {});
    Q_INVOKABLE void refresh();

    static QStringList socketCandidates(const QString &service);

signals:
    void connectedChanged();
    void stateChanged();
    void replied(const QString &tag, bool ok, const QVariant &result);

private:
    void connectToService();
    void onReadyRead();
    void onDisconnected();
    void send(const QString &method, const QVariantMap &params, const QString &tag);
    void handleLine(const QByteArray &line);
    void setConnected(bool on);
    QString stateMethod() const { return m_service == QLatin1String("client") ? QStringLiteral("state") : QStringLiteral("status"); }

    QString m_service;
    QLocalSocket m_socket;
    QTimer m_poll;
    QByteArray m_buffer;
    QHash<quint64, QString> m_pending;
    quint64 m_nextId = 1;
    quint64 m_stateRequest = 0;
    bool m_connected = false;
    QVariantMap m_state;
    int m_candidate = 0;
};
