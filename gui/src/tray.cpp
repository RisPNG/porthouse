#include "tray.h"

#include <QAction>
#include <QMenu>
#include <QPainter>
#include <QPixmap>

#include "controllink.h"
#include "theme.h"

namespace {

QPixmap draw(const Theme *theme, bool active, int size) {
    QPixmap pixmap(size, size);
    pixmap.fill(Qt::transparent);
    QPainter p(&pixmap);
    p.setRenderHint(QPainter::Antialiasing);
    const qreal s = size;

    p.setPen(Qt::NoPen);
    p.setBrush(theme->accent());
    p.drawRoundedRect(QRectF(0, 0, s, s), s * 0.22, s * 0.22);

    // A plug: the head with its two contacts, and the cable.
    p.setBrush(theme->accentText());
    p.drawRoundedRect(QRectF(s * 0.28, s * 0.16, s * 0.44, s * 0.38), s * 0.05, s * 0.05);
    p.drawRect(QRectF(s * 0.44, s * 0.54, s * 0.12, s * 0.30));
    p.setBrush(theme->accent());
    p.drawRect(QRectF(s * 0.37, s * 0.24, s * 0.08, s * 0.10));
    p.drawRect(QRectF(s * 0.55, s * 0.24, s * 0.08, s * 0.10));

    if (active) {
        const qreal d = s * 0.38;
        p.setBrush(theme->surface());
        p.drawEllipse(QRectF(s - d - s * 0.02, s - d - s * 0.02, d, d));
        p.setBrush(theme->good());
        const qreal inner = d * 0.66;
        p.drawEllipse(QRectF(s - d - s * 0.02 + (d - inner) / 2, s - d - s * 0.02 + (d - inner) / 2, inner, inner));
    }
    return pixmap;
}

int count(const QVariant &list) {
    return list.toList().size();
}

}  // namespace

bool Tray::available() {
    return QSystemTrayIcon::isSystemTrayAvailable();
}

QIcon Tray::icon(const Theme *theme, bool active) {
    QIcon icon;
    for (int size : {16, 22, 24, 32, 48, 64, 128, 256}) {
        icon.addPixmap(draw(theme, active, size));
    }
    return icon;
}

Tray::Tray(Theme *theme, ControlLink *client, ControlLink *server, QObject *parent)
    : QObject(parent), m_theme(theme), m_client(client), m_server(server) {
    m_menu = new QMenu();
    QAction *open = m_menu->addAction(tr("Open Porthouse"));
    m_menu->addSeparator();
    m_usingLine = m_menu->addAction(QString());
    m_usingLine->setEnabled(false);
    m_sharingLine = m_menu->addAction(QString());
    m_sharingLine->setEnabled(false);
    m_menu->addSeparator();
    QAction *quit = m_menu->addAction(tr("Quit"));

    connect(open, &QAction::triggered, this, &Tray::openRequested);
    connect(quit, &QAction::triggered, this, &Tray::quitRequested);
    connect(&m_icon, &QSystemTrayIcon::activated, this, [this](QSystemTrayIcon::ActivationReason reason) {
        if (reason == QSystemTrayIcon::Trigger || reason == QSystemTrayIcon::DoubleClick) {
            emit openRequested();
        }
    });
    connect(m_client, &ControlLink::stateChanged, this, &Tray::update);
    connect(m_server, &ControlLink::stateChanged, this, &Tray::update);
    connect(m_theme, &Theme::changed, this, &Tray::update);

    m_icon.setContextMenu(m_menu);
    update();
    m_icon.show();
}

void Tray::update() {
    const int used = count(m_client->state().value(QStringLiteral("attachments")));

    int shared = 0;
    const QVariantList devices = m_server->state().value(QStringLiteral("devices")).toList();
    for (const QVariant &device : devices) {
        if (!device.toMap().value(QStringLiteral("in_use_by")).isNull()) {
            ++shared;
        }
    }

    QString usingText;
    if (!m_client->connected()) {
        usingText = tr("Using: service not running");
    } else if (used == 0) {
        usingText = tr("Using: nothing");
    } else {
        usingText = tr("Using: %n device(s)", nullptr, used);
    }

    QString sharingText;
    if (!m_server->connected()) {
        sharingText = tr("Sharing: service not running");
    } else if (shared == 0) {
        sharingText = tr("Sharing: nothing in use by others");
    } else {
        sharingText = tr("Sharing: %n device(s) in use by others", nullptr, shared);
    }

    m_usingLine->setText(usingText);
    m_sharingLine->setText(sharingText);
    m_icon.setIcon(icon(m_theme, used > 0 || shared > 0));
    m_icon.setToolTip(QStringLiteral("Porthouse\n%1\n%2").arg(usingText, sharingText));
}
