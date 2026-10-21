.pragma library

var speeds = { Low: "1.5 Mb/s", Full: "12 Mb/s", High: "480 Mb/s", Super: "5 Gb/s", SuperPlus: "10 Gb/s", Unknown: "" }

function hex4(n) {
    var s = Number(n).toString(16)
    while (s.length < 4) {
        s = "0" + s
    }
    return s
}

function deviceName(d) {
    var parts = []
    if (d.manufacturer) {
        parts.push(d.manufacturer)
    }
    if (d.product) {
        parts.push(d.product)
    }
    return parts.length > 0 ? parts.join(" ") : qsTr("Unnamed device")
}

function deviceDetails(d) {
    var parts = [d.busid, hex4(d.vendor_id) + ":" + hex4(d.product_id)]
    if (speeds[d.speed]) {
        parts.push(speeds[d.speed])
    }
    return parts.join("  ·  ")
}

function clock(seconds) {
    return Qt.formatTime(new Date(seconds * 1000), "HH:mm:ss")
}

function eventLines(events) {
    var lines = []
    for (var i = events.length - 1; i >= 0; i--) {
        var parts = events[i].split("\t")
        lines.push(clock(Number(parts[0])) + "   " + parts.slice(1).join(" "))
    }
    return lines
}

function countdown(expires, now) {
    var left = Math.max(0, expires - now)
    var m = Math.floor(left / 60)
    var s = left % 60
    return m + ":" + (s < 10 ? "0" : "") + s
}
