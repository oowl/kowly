import QtQuick
import QtQuick.Layouts
import QtQuick.Controls as QQC2
import QtQuick.Dialogs
import QtCore
import org.kde.kirigami as Kirigami
import io.github.owlydrop

Kirigami.ApplicationWindow {
    id: root

    title: "AirDrop"
    width: Kirigami.Units.gridUnit * 30
    height: Kirigami.Units.gridUnit * 40
    minimumWidth: Kirigami.Units.gridUnit * 22
    minimumHeight: Kirigami.Units.gridUnit * 28

    readonly property var devices: JSON.parse(backend.devices)
    readonly property var transfers: JSON.parse(backend.transfers)
    readonly property var requests: JSON.parse(backend.requests)
    readonly property bool radioUp: backend.radioState === "up"
    readonly property bool radioBusy: backend.radioState === "starting" || backend.radioState === "stopping"
    // Files waiting for a device: URL strings.
    property var staged: []

    // "zh" or "en": the setting, or the system's language when it's "auto".
    readonly property string language: settings.language === "auto"
        ? (Qt.locale().name.startsWith("zh") ? "zh" : "en")
        : settings.language

    // The text in the current language. Bindings that call it follow a
    // change of language at once.
    function tr(en, zh) {
        return language === "zh" ? zh : en
    }

    Backend {
        id: backend
        onErrorOccurred: message => root.showPassiveNotification(message, "long")
        // Look around as soon as the radio is up, as a share sheet does.
        onRadioStateChanged: if (radioState === "up") scan()
        // From Dolphin's menu, through another `owly-drop FILE...`.
        onFilesRequested: urls => {
            root.stageLines(urls)
            if (root.staged.length > 0 && radioState === "off")
                radioUp()
            root.show()
            root.raise()
            root.requestActivate()
        }
    }

    Settings {
        id: settings
        property string name: ""
        property string dir: ""
        property bool autoAccept: false
        property bool autoStart: false
        // "auto", "zh" or "en".
        property string language: "auto"
    }

    function applySettings() {
        backend.configure(settings.name, settings.dir, settings.autoAccept, language)
    }

    onLanguageChanged: applySettings()

    function stage(urls) {
        let next = staged.slice()
        for (let i = 0; i < urls.length; ++i) {
            const url = urls[i].toString()
            if (url.startsWith("file:") && next.indexOf(url) < 0)
                next.push(url)
        }
        staged = next
        if (radioUp && devices.length === 0 && !backend.scanning)
            backend.scan()
    }

    // URLs, one per line, as the backend hands them over.
    function stageLines(text) {
        const urls = text.split("\n").filter(u => u.length > 0)
        if (urls.length > 0)
            stage(urls)
    }

    function unstage(url) {
        staged = staged.filter(u => u !== url)
    }

    function sendTo(device, urls) {
        if (!radioUp) {
            showPassiveNotification(tr("Turn AirDrop on first", "先打开 AirDrop"))
            return
        }
        if (urls.length === 0) {
            showPassiveNotification(tr("Drop files here first, or click “Choose Files”",
                                       "先把文件拖进来，或点「选择文件」"))
            return
        }
        backend.sendFiles(device.id, urls.join("\n"))
        if (urls === staged)
            staged = []
    }

    function fileName(url) {
        const path = backend.localPath(url)
        return path.substring(path.lastIndexOf("/") + 1)
    }

    function formatSize(bytes) {
        if (bytes === undefined || bytes === null)
            return ""
        const units = ["B", "KB", "MB", "GB"]
        let i = 0
        let n = bytes
        while (n >= 1024 && i < units.length - 1) {
            n /= 1024
            ++i
        }
        return (i === 0 ? n : n.toFixed(1)) + " " + units[i]
    }

    function phaseText(t) {
        const sending = t.direction === "send"
        switch (t.phase) {
        case "waking": return tr("Waking the receiver…", "正在唤醒对方…")
        case "asking": return sending ? tr("Waiting for them to accept…", "等待对方接受…")
                                      : tr("Waiting for you", "等待你确认")
        case "transferring": return sending ? tr("Sending", "正在发送") : tr("Receiving", "正在接收")
        case "done": return sending ? tr("Sent", "已发送") : tr("Received", "已接收")
        case "declined": return tr("Declined", "已拒绝")
        case "withdrawn": return tr("Cancelled", "已取消")
        case "failed": return tr("Failed", "失败")
        }
        return t.phase
    }

    function radioText() {
        switch (backend.radioState) {
        case "starting": return tr("Starting, taking over the Wi-Fi card…", "正在启动，接管无线网卡…")
        case "stopping": return tr("Stopping, giving the Wi-Fi card back…", "正在关闭，归还无线网卡…")
        case "up":
            return tr("On", "已开启") + " · " + backend.radioDetail
                + (backend.manual ? "" : " · " + tr("AWDL peers nearby: ", "附近 AWDL 设备 ") + backend.peerCount)
        }
        return backend.radioDetail.length > 0
            ? tr("Off: ", "已关闭：") + backend.radioDetail
            : tr("Off", "已关闭")
    }

    onActiveChanged: backend.setWindowActive(active)

    Component.onCompleted: {
        if (settings.name.length === 0)
            settings.name = backend.defaultName()
        if (settings.dir.length === 0)
            settings.dir = backend.defaultDir()
        const lang = devArg("--dev-lang")
        if (lang)
            settings.language = lang
        const name = devArg("--dev-name")
        if (name)
            settings.name = name
        applySettings()
        backend.setWindowActive(active)
        stageLines(backend.takeLaunchFiles())
        if (settings.autoStart || devArg("--dev-up") !== undefined || staged.length > 0)
            backend.radioUp()
        const file = devArg("--dev-stage")
        if (file)
            stage(["file://" + file])
        const shot = devArg("--dev-screenshot")
        if (shot)
            screenshotTimer.start()
    }

    // For testing: `--dev-up`, `--dev-stage FILE`, `--dev-send-to NAME`,
    // `--dev-lang zh|en`, `--dev-name NAME` and `--dev-screenshot PATH`
    // (taken after `--dev-wait` ms).
    function devArg(name) {
        const args = Qt.application.arguments
        const i = args.indexOf(name)
        if (i < 0)
            return undefined
        return i + 1 < args.length && !args[i + 1].startsWith("--") ? args[i + 1] : ""
    }

    onDevicesChanged: {
        const to = devArg("--dev-send-to")
        if (to && staged.length > 0)
            for (const d of devices)
                if (d.name === to)
                    sendTo(d, staged)
    }

    Timer {
        id: screenshotTimer
        interval: Number(root.devArg("--dev-wait") || 3000)
        onTriggered: root.pageStack.grabToImage(result => {
            result.saveToFile(root.devArg("--dev-screenshot"))
            Qt.quit()
        })
    }

    FileDialog {
        id: fileDialog
        title: root.tr("Choose Files to Send", "选择要发送的文件")
        fileMode: FileDialog.OpenFiles
        onAccepted: root.stage(selectedFiles)
    }

    FolderDialog {
        id: folderDialog
        title: root.tr("Save Received Files To", "接收的文件保存到")
        onAccepted: dirField.text = backend.localPath(selectedFolder.toString())
    }

    QQC2.Dialog {
        id: settingsDialog
        title: root.tr("Settings", "设置")
        modal: true
        anchors.centerIn: QQC2.Overlay.overlay
        width: Math.min(root.width - Kirigami.Units.gridUnit * 2, Kirigami.Units.gridUnit * 26)
        standardButtons: QQC2.Dialog.Ok | QQC2.Dialog.Cancel

        onAboutToShow: {
            nameField.text = settings.name
            dirField.text = settings.dir
            autoAcceptBox.checked = settings.autoAccept
            autoStartBox.checked = settings.autoStart
            languageBox.currentIndex = languageBox.indexOfValue(settings.language)
        }
        onAccepted: {
            settings.name = nameField.text.trim().length > 0 ? nameField.text.trim() : backend.defaultName()
            settings.dir = dirField.text.trim().length > 0 ? dirField.text.trim() : backend.defaultDir()
            settings.autoAccept = autoAcceptBox.checked
            settings.autoStart = autoStartBox.checked
            settings.language = languageBox.currentValue
            root.applySettings()
        }

        Kirigami.FormLayout {
            anchors.fill: parent

            QQC2.TextField {
                id: nameField
                Kirigami.FormData.label: root.tr("Name:", "显示名称：")
                placeholderText: backend.defaultName()
            }
            RowLayout {
                Kirigami.FormData.label: root.tr("Save to:", "保存到：")
                QQC2.TextField {
                    id: dirField
                    Layout.fillWidth: true
                }
                QQC2.Button {
                    icon.name: "document-open-folder"
                    onClicked: folderDialog.open()
                    QQC2.ToolTip.text: root.tr("Choose Folder", "选择文件夹")
                    QQC2.ToolTip.visible: hovered
                }
            }
            QQC2.CheckBox {
                id: autoAcceptBox
                Kirigami.FormData.label: root.tr("Receiving:", "接收：")
                text: root.tr("Accept all files without asking", "自动接受所有文件（不询问）")
            }
            QQC2.CheckBox {
                id: autoStartBox
                Kirigami.FormData.label: root.tr("Startup:", "启动：")
                text: root.tr("Turn AirDrop on when the app opens", "打开程序时自动开启 AirDrop")
            }
            QQC2.ComboBox {
                id: languageBox
                Kirigami.FormData.label: root.tr("Language:", "语言：")
                // Fixed: Breeze sizes a combo box by its items' text, which
                // changes with the language, and loops.
                implicitWidth: Kirigami.Units.gridUnit * 9
                textRole: "text"
                valueRole: "value"
                // Each language names itself, as language menus do.
                model: [
                    { text: root.tr("System", "跟随系统"), value: "auto" },
                    { text: "中文", value: "zh" },
                    { text: "English", value: "en" },
                ]
            }
        }
    }

    pageStack.initialPage: Kirigami.ScrollablePage {
        id: page
        title: "AirDrop"

        actions: [
            Kirigami.Action {
                text: root.tr("Settings", "设置")
                icon.name: "configure"
                onTriggered: settingsDialog.open()
            }
        ]

        ColumnLayout {
            spacing: Kirigami.Units.largeSpacing

            // The radio.
            Kirigami.AbstractCard {
                Layout.fillWidth: true
                contentItem: RowLayout {
                    spacing: Kirigami.Units.largeSpacing
                    Kirigami.Icon {
                        source: root.radioUp ? "network-wireless-hotspot" : "network-wireless-disconnected"
                        Layout.preferredWidth: Kirigami.Units.iconSizes.large
                        Layout.preferredHeight: Kirigami.Units.iconSizes.large
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0
                        Kirigami.Heading {
                            level: 3
                            text: root.tr("Visible as “" + settings.name + "”",
                                          "以「" + settings.name + "」可被发现")
                            elide: Text.ElideRight
                            Layout.fillWidth: true
                        }
                        QQC2.Label {
                            text: root.radioText()
                            wrapMode: Text.Wrap
                            Layout.fillWidth: true
                            color: backend.radioState === "off" && backend.radioDetail.length > 0
                                ? Kirigami.Theme.negativeTextColor : Kirigami.Theme.disabledTextColor
                        }
                    }
                    QQC2.BusyIndicator {
                        running: root.radioBusy
                        visible: running
                        Layout.preferredHeight: Kirigami.Units.iconSizes.medium
                    }
                    QQC2.Switch {
                        checked: root.radioUp || backend.radioState === "starting"
                        enabled: !root.radioBusy
                        onToggled: checked ? backend.radioUp() : backend.radioDown()
                    }
                }
            }

            // Incoming requests.
            Repeater {
                model: root.requests
                delegate: Kirigami.AbstractCard {
                    required property var modelData
                    Layout.fillWidth: true
                    highlighted: true
                    contentItem: ColumnLayout {
                        spacing: Kirigami.Units.smallSpacing
                        RowLayout {
                            Kirigami.Icon {
                                source: "document-save"
                                Layout.preferredWidth: Kirigami.Units.iconSizes.medium
                                Layout.preferredHeight: Kirigami.Units.iconSizes.medium
                            }
                            Kirigami.Heading {
                                level: 3
                                text: root.tr(modelData.sender + " wants to send you",
                                              modelData.sender + " 想给你发送")
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                        }
                        Repeater {
                            model: modelData.items
                            delegate: QQC2.Label {
                                required property var modelData
                                text: "• " + modelData.name + (modelData.size ? "  (" + root.formatSize(modelData.size) + ")" : "")
                                elide: Text.ElideMiddle
                                Layout.fillWidth: true
                            }
                        }
                        Repeater {
                            model: modelData.urls
                            delegate: QQC2.Label {
                                required property var modelData
                                text: "🔗 " + modelData
                                elide: Text.ElideMiddle
                                Layout.fillWidth: true
                            }
                        }
                        RowLayout {
                            Layout.alignment: Qt.AlignRight
                            QQC2.Button {
                                text: root.tr("Decline", "拒绝")
                                icon.name: "dialog-cancel"
                                onClicked: backend.answer(modelData.id, false)
                            }
                            QQC2.Button {
                                text: root.tr("Accept", "接受")
                                icon.name: "dialog-ok-apply"
                                highlighted: true
                                onClicked: backend.answer(modelData.id, true)
                            }
                        }
                    }
                }
            }

            // Where files are dropped.
            Rectangle {
                id: dropZone
                Layout.fillWidth: true
                Layout.preferredHeight: dropColumn.implicitHeight + Kirigami.Units.gridUnit * 2
                radius: Kirigami.Units.cornerRadius
                color: dropArea.containsDrag
                    ? Qt.alpha(Kirigami.Theme.highlightColor, 0.15)
                    : Qt.alpha(Kirigami.Theme.textColor, 0.03)
                border.width: 2
                border.color: dropArea.containsDrag
                    ? Kirigami.Theme.highlightColor
                    : Qt.alpha(Kirigami.Theme.textColor, 0.2)

                DropArea {
                    id: dropArea
                    anchors.fill: parent
                    onEntered: drag => drag.accepted = drag.hasUrls
                    onDropped: drop => {
                        if (drop.hasUrls) {
                            root.stage(drop.urls)
                            drop.acceptProposedAction()
                        }
                    }
                }

                ColumnLayout {
                    id: dropColumn
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.margins: Kirigami.Units.gridUnit
                    spacing: Kirigami.Units.smallSpacing

                    Kirigami.Icon {
                        source: "document-send"
                        Layout.alignment: Qt.AlignHCenter
                        Layout.preferredWidth: Kirigami.Units.iconSizes.huge
                        Layout.preferredHeight: Kirigami.Units.iconSizes.huge
                        visible: root.staged.length === 0
                    }
                    QQC2.Label {
                        text: root.staged.length === 0
                            ? root.tr("Drop files here", "把文件拖到这里")
                            : root.tr(root.staged.length + " file(s) ready: click a device below to send",
                                      "待发送 " + root.staged.length + " 个文件，点下面的设备发送")
                        font.bold: root.staged.length > 0
                        horizontalAlignment: Text.AlignHCenter
                        wrapMode: Text.Wrap
                        Layout.fillWidth: true
                    }
                    Repeater {
                        model: root.staged
                        delegate: RowLayout {
                            required property var modelData
                            Layout.fillWidth: true
                            Kirigami.Icon {
                                source: "text-x-generic"
                                Layout.preferredWidth: Kirigami.Units.iconSizes.small
                                Layout.preferredHeight: Kirigami.Units.iconSizes.small
                            }
                            QQC2.Label {
                                text: root.fileName(modelData)
                                elide: Text.ElideMiddle
                                Layout.fillWidth: true
                            }
                            QQC2.ToolButton {
                                icon.name: "list-remove"
                                onClicked: root.unstage(modelData)
                                QQC2.ToolTip.text: root.tr("Remove", "移除")
                                QQC2.ToolTip.visible: hovered
                            }
                        }
                    }
                    RowLayout {
                        Layout.alignment: Qt.AlignHCenter
                        QQC2.Button {
                            text: root.staged.length === 0
                                ? root.tr("Choose Files…", "选择文件…")
                                : root.tr("Add Files…", "添加文件…")
                            icon.name: "list-add"
                            onClicked: fileDialog.open()
                        }
                        QQC2.Button {
                            text: root.tr("Clear", "清空")
                            icon.name: "edit-clear-all"
                            visible: root.staged.length > 0
                            onClicked: root.staged = []
                        }
                    }
                }
            }

            // Devices.
            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: Kirigami.Units.largeSpacing
                Kirigami.Heading {
                    level: 2
                    text: root.tr("Nearby", "附近的设备")
                    Layout.fillWidth: true
                }
                QQC2.BusyIndicator {
                    running: backend.scanning
                    visible: running
                    Layout.preferredHeight: Kirigami.Units.iconSizes.medium
                }
                QQC2.Button {
                    text: backend.scanning ? root.tr("Scanning…", "扫描中…") : root.tr("Scan", "扫描")
                    icon.name: "view-refresh"
                    enabled: root.radioUp && !backend.scanning
                    onClicked: backend.scan()
                }
            }

            Kirigami.PlaceholderMessage {
                Layout.fillWidth: true
                visible: root.devices.length === 0
                icon.name: root.radioUp ? "smartphone" : "network-wireless-disconnected"
                text: root.radioUp
                    ? (backend.scanning ? root.tr("Looking…", "正在寻找…") : root.tr("No devices found yet", "还没有发现设备"))
                    : root.tr("AirDrop is off", "AirDrop 未开启")
                explanation: root.radioUp
                    ? root.tr("Unlock the iPhone and set its AirDrop to Everyone for 10 Minutes, then click Scan.",
                              "确保 iPhone 已解锁，并把 AirDrop 设为「所有人（10 分钟）」，然后点「扫描」。")
                    : root.tr("Turn on the switch above to send and receive.", "打开上面的开关后才能收发文件。")
            }

            Flow {
                Layout.fillWidth: true
                spacing: Kirigami.Units.largeSpacing
                visible: root.devices.length > 0

                Repeater {
                    model: root.devices
                    delegate: QQC2.AbstractButton {
                        id: deviceButton
                        required property var modelData
                        width: Kirigami.Units.gridUnit * 7
                        height: Kirigami.Units.gridUnit * 7
                        hoverEnabled: true
                        onClicked: root.sendTo(modelData, root.staged)

                        QQC2.ToolTip.text: root.staged.length > 0
                            ? root.tr("Send " + root.staged.length + " file(s) to " + modelData.name,
                                      "把 " + root.staged.length + " 个文件发给 " + modelData.name)
                            : root.tr("Drop files here to send them to " + modelData.name,
                                      "把文件拖到这里，直接发给 " + modelData.name)
                        QQC2.ToolTip.visible: hovered
                        QQC2.ToolTip.delay: Kirigami.Units.toolTipDelay

                        background: Rectangle {
                            radius: Kirigami.Units.cornerRadius
                            color: deviceDrop.containsDrag || deviceButton.hovered
                                ? Qt.alpha(Kirigami.Theme.highlightColor, 0.2)
                                : Qt.alpha(Kirigami.Theme.textColor, 0.05)
                            border.width: deviceDrop.containsDrag ? 2 : 0
                            border.color: Kirigami.Theme.highlightColor
                        }

                        contentItem: ColumnLayout {
                            spacing: Kirigami.Units.smallSpacing
                            Kirigami.Icon {
                                source: modelData.model.indexOf("Mac") >= 0 ? "computer-laptop" : "smartphone"
                                Layout.alignment: Qt.AlignHCenter
                                Layout.preferredWidth: Kirigami.Units.iconSizes.large
                                Layout.preferredHeight: Kirigami.Units.iconSizes.large
                            }
                            QQC2.Label {
                                text: modelData.name
                                horizontalAlignment: Text.AlignHCenter
                                wrapMode: Text.Wrap
                                maximumLineCount: 2
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }
                        }

                        DropArea {
                            id: deviceDrop
                            anchors.fill: parent
                            onEntered: drag => drag.accepted = drag.hasUrls
                            onDropped: drop => {
                                if (!drop.hasUrls)
                                    return
                                let urls = []
                                for (let i = 0; i < drop.urls.length; ++i)
                                    urls.push(drop.urls[i].toString())
                                root.sendTo(deviceButton.modelData, urls)
                                drop.acceptProposedAction()
                            }
                        }
                    }
                }
            }

            // Transfers.
            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: Kirigami.Units.largeSpacing
                visible: root.transfers.length > 0
                Kirigami.Heading {
                    level: 2
                    text: root.tr("Transfers", "传输记录")
                    Layout.fillWidth: true
                }
                QQC2.Button {
                    text: root.tr("Clear Finished", "清除已完成")
                    icon.name: "edit-clear-history"
                    onClicked: backend.clearFinished()
                }
            }

            Repeater {
                model: root.transfers
                delegate: Kirigami.AbstractCard {
                    id: transferCard
                    required property var modelData
                    readonly property bool active: modelData.phase === "waking"
                        || modelData.phase === "asking" || modelData.phase === "transferring"
                    readonly property bool bad: modelData.phase === "failed" || modelData.phase === "declined"
                        || modelData.phase === "withdrawn"
                    Layout.fillWidth: true

                    contentItem: ColumnLayout {
                        spacing: Kirigami.Units.smallSpacing
                        RowLayout {
                            spacing: Kirigami.Units.largeSpacing
                            Kirigami.Icon {
                                source: modelData.direction === "send" ? "document-send" : "document-save"
                                Layout.preferredWidth: Kirigami.Units.iconSizes.medium
                                Layout.preferredHeight: Kirigami.Units.iconSizes.medium
                            }
                            ColumnLayout {
                                spacing: 0
                                Layout.fillWidth: true
                                QQC2.Label {
                                    text: modelData.title.length > 0 ? modelData.title : "…"
                                    font.bold: true
                                    elide: Text.ElideMiddle
                                    Layout.fillWidth: true
                                }
                                QQC2.Label {
                                    text: (modelData.direction === "send"
                                            ? root.tr("To ", "发给 ") : root.tr("From ", "来自 "))
                                        + modelData.peer
                                        + " · " + root.phaseText(modelData)
                                        + (modelData.total > 0 && transferCard.active
                                            ? " · " + root.formatSize(modelData.sent) + " / " + root.formatSize(modelData.total)
                                            : "")
                                    color: transferCard.bad ? Kirigami.Theme.negativeTextColor
                                        : modelData.phase === "done" ? Kirigami.Theme.positiveTextColor
                                        : Kirigami.Theme.disabledTextColor
                                    elide: Text.ElideRight
                                    Layout.fillWidth: true
                                }
                            }
                            QQC2.ToolButton {
                                visible: modelData.direction === "receive" && modelData.phase === "done"
                                    && modelData.paths.length === 1
                                icon.name: "document-open"
                                onClicked: backend.openPath(modelData.paths[0])
                                QQC2.ToolTip.text: root.tr("Open", "打开")
                                QQC2.ToolTip.visible: hovered
                            }
                            QQC2.ToolButton {
                                visible: modelData.direction === "receive" && modelData.phase === "done"
                                    && modelData.paths.length > 0
                                icon.name: "folder-open"
                                onClicked: backend.revealPath(modelData.paths[0])
                                QQC2.ToolTip.text: root.tr("Show in Folder", "在文件夹中显示")
                                QQC2.ToolTip.visible: hovered
                            }
                        }
                        QQC2.ProgressBar {
                            Layout.fillWidth: true
                            visible: transferCard.active
                            indeterminate: modelData.phase !== "transferring" || modelData.total === 0
                            from: 0
                            to: Math.max(modelData.total, 1)
                            value: modelData.sent
                        }
                        QQC2.Label {
                            visible: modelData.detail.length > 0 && transferCard.bad
                            text: modelData.detail
                            wrapMode: Text.Wrap
                            color: Kirigami.Theme.negativeTextColor
                            Layout.fillWidth: true
                        }
                        Repeater {
                            model: modelData.urls
                            delegate: QQC2.Button {
                                required property var modelData
                                text: modelData
                                icon.name: "internet-web-browser"
                                flat: true
                                Layout.fillWidth: true
                                onClicked: Qt.openUrlExternally(modelData)
                            }
                        }
                    }
                }
            }

            Item {
                Layout.fillHeight: true
            }
        }
    }
}
