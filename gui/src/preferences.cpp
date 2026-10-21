#include "preferences.h"

#include <QSettings>

Preferences::Preferences(QObject *parent) : QObject(parent) {}

bool Preferences::confirmInputDevices() const {
    return QSettings().value(QStringLiteral("confirmInputDevices"), true).toBool();
}

void Preferences::setConfirmInputDevices(bool on) {
    QSettings().setValue(QStringLiteral("confirmInputDevices"), on);
    emit changed();
}
