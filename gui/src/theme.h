#pragma once

#include <QColor>
#include <QObject>

// Every colour the program uses lives here, for the light and the dark appearance.
class Theme : public QObject {
    Q_OBJECT
    Q_PROPERTY(bool dark READ dark NOTIFY changed)
    Q_PROPERTY(QColor background READ background NOTIFY changed)
    Q_PROPERTY(QColor surface READ surface NOTIFY changed)
    Q_PROPERTY(QColor border READ border NOTIFY changed)
    Q_PROPERTY(QColor text READ text NOTIFY changed)
    Q_PROPERTY(QColor textMuted READ textMuted NOTIFY changed)
    Q_PROPERTY(QColor accent READ accent NOTIFY changed)
    Q_PROPERTY(QColor accentText READ accentText NOTIFY changed)
    Q_PROPERTY(QColor good READ good NOTIFY changed)
    Q_PROPERTY(QColor warning READ warning NOTIFY changed)
    Q_PROPERTY(QColor bad READ bad NOTIFY changed)
    Q_PROPERTY(QColor chip READ chip NOTIFY changed)
    Q_PROPERTY(QColor overlay READ overlay NOTIFY changed)

public:
    explicit Theme(QObject *parent = nullptr);

    bool dark() const { return m_dark; }
    QColor background() const { return pick(0xf6f7f9, 0x0f1218); }
    QColor surface() const { return pick(0xffffff, 0x171b23); }
    QColor border() const { return pick(0xe2e5ea, 0x272d38); }
    QColor text() const { return pick(0x1b1f27, 0xe8ebf1); }
    QColor textMuted() const { return pick(0x6a7384, 0x8f99ab); }
    QColor accent() const { return pick(0x2563eb, 0x5b8def); }
    QColor accentText() const { return pick(0xffffff, 0x0b0e14); }
    QColor good() const { return pick(0x15803d, 0x4ade80); }
    QColor warning() const { return pick(0xb45309, 0xfbbf24); }
    QColor bad() const { return pick(0xb91c1c, 0xf87171); }
    QColor chip() const { return pick(0xeef1f6, 0x212736); }
    QColor overlay() const { return pick(0x66000000, 0x99000000); }

    bool eventFilter(QObject *watched, QEvent *event) override;

signals:
    void changed();

private:
    QColor pick(quint32 light, quint32 dark) const;
    void update();

    bool m_dark = false;
};
