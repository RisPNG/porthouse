#include "theme.h"

#include <QEvent>
#include <QGuiApplication>
#include <QPalette>

Theme::Theme(QObject *parent) : QObject(parent) {
    update();
    qApp->installEventFilter(this);
}

QColor Theme::pick(quint32 light, quint32 dark) const {
    const quint32 value = m_dark ? dark : light;
    // Values above 24 bits carry an alpha channel.
    return value > 0xffffff ? QColor::fromRgba(value) : QColor::fromRgb(value);
}

bool Theme::eventFilter(QObject *watched, QEvent *event) {
    if (watched == qApp && event->type() == QEvent::ApplicationPaletteChange) {
        update();
    }
    return QObject::eventFilter(watched, event);
}

void Theme::update() {
    const bool dark = QGuiApplication::palette().color(QPalette::Window).lightness() < 128;
    if (dark != m_dark) {
        m_dark = dark;
        emit changed();
    }
}
