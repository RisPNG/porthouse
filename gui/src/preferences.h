#pragma once

#include <QObject>

// Choices the person made in the program, kept between runs.
class Preferences : public QObject {
    Q_OBJECT
    Q_PROPERTY(bool confirmInputDevices READ confirmInputDevices WRITE setConfirmInputDevices NOTIFY changed)

public:
    explicit Preferences(QObject *parent = nullptr);

    bool confirmInputDevices() const;
    void setConfirmInputDevices(bool on);

signals:
    void changed();
};
