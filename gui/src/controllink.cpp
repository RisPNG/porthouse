#include "controllink.h"

#include <QDir>
#include <QJsonDocument>
#include <QJsonObject>

ControlLink::ControlLink(const QString &service, QObject *parent) : QObject(parent), m_service(service) {
    connect(&m_socket, &QLocalSocket::connected, this, [this] {
        setConnected(true);
        refresh();
    });
    connect(&m_socket, &QLocalSocket::readyRead, this, &ControlLink::onReadyRead);
    connect(&m_socket, &QLocalSocket::disconnected, this, &ControlLink::onDisconnected);
    connect(&m_socket, &QLocalSocket::errorOccurred, this, [this] {
        if (m_socket.state() == QLocalSocket::UnconnectedState) {
            onDisconnected();
        }
    });
    connect(&m_poll, &QTimer::timeout, this, [this] {
        if (m_socket.state() == QLocalSocket::ConnectedState) {
            refresh();
        } else if (m_socket.state() == QLocalSocket::UnconnectedState) {
            connectToService();
        }
    });
}

ControlLink::~ControlLink() {
    // The socket reports its own closing; the members its handlers use are already gone by then.
    m_socket.disconnect(this);
    m_socket.abort();
}

QStringList ControlLink::socketCandidates(const QString &service) {
#ifdef Q_OS_WIN
    return {QStringLiteral("porthouse-") + service};
#else
    const QString file = service + QStringLiteral(".sock");
    const QByteArray override = qgetenv("PORTHOUSE_IPC_DIR");
    if (!override.isEmpty()) {
        return {QDir(QString::fromLocal8Bit(override)).filePath(file)};
    }
    QStringList list{QStringLiteral("/run/porthouse/") + file};
    const QByteArray runtime = qgetenv("XDG_RUNTIME_DIR");
    if (!runtime.isEmpty()) {
        list << QDir(QString::fromLocal8Bit(runtime)).filePath(QStringLiteral("porthouse/") + file);
    }
    return list;
#endif
}

void ControlLink::start(int pollMs) {
    connectToService();
    m_poll.start(pollMs);
}

void ControlLink::connectToService() {
    const QStringList candidates = socketCandidates(m_service);
    m_candidate = (m_candidate + 1) % candidates.size();
    m_socket.abort();
    m_socket.setServerName(candidates.at(m_candidate));
    m_socket.connectToServer();
}

void ControlLink::refresh() {
    if (m_socket.state() != QLocalSocket::ConnectedState || m_stateRequest != 0) {
        return;
    }
    QVariantMap params;
    if (m_service == QLatin1String("client")) {
        params.insert(QStringLiteral("refresh"), false);
    }
    send(stateMethod(), params, QStringLiteral("\x01state"));
}

void ControlLink::call(const QString &method, const QVariantMap &params, const QString &tag) {
    if (m_socket.state() != QLocalSocket::ConnectedState) {
        emit replied(tag, false, tr("The Porthouse %1 service is not running.").arg(m_service));
        return;
    }
    send(method, params, tag);
}

void ControlLink::send(const QString &method, const QVariantMap &params, const QString &tag) {
    const quint64 id = m_nextId++;
    m_pending.insert(id, tag);
    if (tag == QLatin1String("\x01state")) {
        m_stateRequest = id;
    }
    const QJsonObject request{{QStringLiteral("id"), static_cast<double>(id)},
                              {QStringLiteral("method"), method},
                              {QStringLiteral("params"), QJsonObject::fromVariantMap(params)}};
    m_socket.write(QJsonDocument(request).toJson(QJsonDocument::Compact) + '\n');
}

void ControlLink::onReadyRead() {
    m_buffer += m_socket.readAll();
    for (;;) {
        const int end = m_buffer.indexOf('\n');
        if (end < 0) {
            break;
        }
        const QByteArray line = m_buffer.left(end);
        m_buffer.remove(0, end + 1);
        handleLine(line);
    }
}

void ControlLink::handleLine(const QByteArray &line) {
    const QJsonObject reply = QJsonDocument::fromJson(line).object();
    const quint64 id = static_cast<quint64>(reply.value(QStringLiteral("id")).toDouble());
    if (!m_pending.contains(id)) {
        return;
    }
    const QString tag = m_pending.take(id);
    const bool ok = !reply.contains(QStringLiteral("error"));
    if (tag == QLatin1String("\x01state")) {
        m_stateRequest = 0;
        if (ok) {
            m_state = reply.value(QStringLiteral("result")).toObject().toVariantMap();
            emit stateChanged();
        }
        return;
    }
    emit replied(tag, ok, ok ? reply.value(QStringLiteral("result")).toVariant() : QVariant(reply.value(QStringLiteral("error")).toString()));
    refresh();
}

void ControlLink::onDisconnected() {
    m_buffer.clear();
    m_pending.clear();
    m_stateRequest = 0;
    setConnected(false);
}

void ControlLink::setConnected(bool on) {
    if (m_connected == on) {
        return;
    }
    m_connected = on;
    if (!on) {
        m_state.clear();
        emit stateChanged();
    }
    emit connectedChanged();
}
