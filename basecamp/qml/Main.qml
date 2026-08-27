// LP-0002 anonymous multisig voting. Basecamp module UI. Binds to the `backend`
// context property (MsigBackend), which drives the localhost Node sidecar
// (msig-sidecar.mjs) over HTTP. Three sections:
//   1. Derive my leaf   (local SHA256("/lp0002/leaf/\x00"||secret); secret stays local).
//   2. Proposal status  (sidecar /status -> proposal_id, member_root, count/threshold).
//   3. Cast anonymous vote: sidecar /approve spawns run_approve_secret (~134s prove),
//      returns the real tx hash + updated count; rejects non-members / double votes.
import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15

Item {
    id: root

    readonly property color colBg: "#0f1117"
    readonly property color colSurface: "#1a1d27"
    readonly property color colBorder: "#2d3148"
    readonly property color colPrimary: "#7c6ef5"
    readonly property color colOk: "#3ecf8e"
    readonly property color colErr: "#e05252"
    readonly property color colText: "#e8e9f0"
    readonly property color colMuted: "#6b7280"

    Connections {
        target: backend
        function onOperationSuccess(op, detail) { toast.flash("✓ " + op + (detail ? " · " + detail : ""), root.colOk) }
        function onOperationError(op, err) { toast.flash("✗ " + op + ": " + err, root.colErr) }
    }

    Component.onCompleted: backend.getStatus()

    Rectangle {
        anchors.fill: parent
        color: root.colBg

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 16
            spacing: 14

            // ── Header ───────────────────────────────────────────────────
            RowLayout {
                Layout.fillWidth: true
                Text { text: "Private Multisig: anonymous M-of-N vote"; color: root.colText; font.pixelSize: 19; font.bold: true }
                Item { Layout.fillWidth: true }
                BusyIndicator { running: backend.busy; visible: backend.busy; implicitWidth: 22; implicitHeight: 22 }
            }
            Text {
                Layout.fillWidth: true
                color: root.colMuted; font.pixelSize: 11; wrapMode: Text.Wrap
                text: "LP-0002 · prove membership in a Merkle set and cast a proposal-bound anonymous on-chain approval. The Vote button drives a REAL zk approval through the sidecar (no static placeholder)."
            }

            // ── Sidecar config ───────────────────────────────────────────
            RowLayout {
                Layout.fillWidth: true; spacing: 8
                Text { text: "Sidecar"; color: root.colMuted; font.pixelSize: 12 }
                TextField {
                    Layout.fillWidth: true; text: backend.sidecarUrl; color: root.colText
                    onEditingFinished: backend.setSidecarUrl(text)
                    background: Rectangle { color: root.colSurface; border.color: root.colBorder; radius: 6 }
                }
            }

            // ════ SECTION 1: Derive my leaf (local) ═════════════════════
            Rectangle {
                Layout.fillWidth: true
                color: root.colSurface; border.color: root.colBorder; radius: 8
                implicitHeight: deriveCol.implicitHeight + 20
                ColumnLayout {
                    id: deriveCol
                    anchors.fill: parent; anchors.margins: 12; spacing: 8
                    Text { text: "1 · Derive my leaf (local)"; color: root.colText; font.pixelSize: 14; font.bold: true }
                    Text {
                        Layout.fillWidth: true; color: root.colMuted; font.pixelSize: 11; wrapMode: Text.Wrap
                        text: "leaf = SHA256(\"/lp0002/leaf/\\x00\" || secret), computed in-process. Your 32-byte secret is hashed locally and never leaves this widget for derivation."
                    }
                    RowLayout {
                        Layout.fillWidth: true; spacing: 8
                        TextField {
                            id: secretField; Layout.fillWidth: true; color: root.colText
                            placeholderText: "member secret, 64 hex chars (e.g. a7a7…a7)"
                            echoMode: TextInput.Normal
                            background: Rectangle { color: root.colBg; border.color: root.colBorder; radius: 6 }
                        }
                        FButton {
                            text: "Derive leaf"
                            enabled: secretField.text.length > 0
                            onClicked: backend.deriveLeaf(secretField.text)
                        }
                    }
                    RowLayout {
                        visible: !!backend.lastLeaf
                        Layout.fillWidth: true; spacing: 8
                        Text { text: "leaf:"; color: root.colMuted; font.pixelSize: 11 }
                        TextEdit {
                            Layout.fillWidth: true
                            text: backend.lastLeaf
                            color: root.colOk; font.family: "monospace"; font.pixelSize: 11
                            readOnly: true; selectByMouse: true; wrapMode: TextEdit.WrapAnywhere
                        }
                    }
                }
            }

            // ════ SECTION 2: Proposal status ════════════════════════════
            Rectangle {
                Layout.fillWidth: true
                color: root.colSurface; border.color: root.colBorder; radius: 8
                implicitHeight: statusCol.implicitHeight + 20
                ColumnLayout {
                    id: statusCol
                    anchors.fill: parent; anchors.margins: 12; spacing: 8
                    RowLayout {
                        Layout.fillWidth: true
                        Text { text: "2 · Proposal status"; color: root.colText; font.pixelSize: 14; font.bold: true }
                        Item { Layout.fillWidth: true }
                        FButton { text: "Refresh status"; enabled: !backend.busy; onClicked: backend.getStatus() }
                    }
                    Text {
                        visible: !backend.statusReady
                        color: root.colMuted; font.pixelSize: 12
                        text: "Proposal not created yet (run deploy → enroll → create_proposal on the target sequencer)."
                    }
                    GridLayout {
                        visible: backend.statusReady
                        Layout.fillWidth: true; columns: 2; columnSpacing: 12; rowSpacing: 4
                        Text { text: "proposal id"; color: root.colMuted; font.pixelSize: 11 }
                        TextEdit {
                            text: backend.proposalId; color: root.colText; font.family: "monospace"; font.pixelSize: 11
                            readOnly: true; selectByMouse: true; Layout.fillWidth: true; wrapMode: TextEdit.WrapAnywhere
                        }
                        Text { text: "member root"; color: root.colMuted; font.pixelSize: 11 }
                        TextEdit {
                            text: backend.memberRoot; color: root.colText; font.family: "monospace"; font.pixelSize: 11
                            readOnly: true; selectByMouse: true; Layout.fillWidth: true; wrapMode: TextEdit.WrapAnywhere
                        }
                    }
                    // Approval progress bar (count / threshold).
                    RowLayout {
                        visible: backend.statusReady
                        Layout.fillWidth: true; spacing: 8
                        Text {
                            color: backend.approvalCount >= backend.threshold ? root.colOk : root.colMuted
                            font.pixelSize: 13; font.bold: true
                            text: "approvals " + backend.approvalCount + " / " + backend.threshold
                                  + (backend.approvalCount >= backend.threshold ? "  ·  THRESHOLD MET" : "")
                        }
                        Item { Layout.fillWidth: true }
                        Repeater {
                            model: Math.max(backend.threshold, 1)
                            Rectangle {
                                width: 22; height: 22; radius: 4
                                color: index < backend.approvalCount ? "#1f3a2a" : "transparent"
                                border.color: index < backend.approvalCount ? root.colOk : root.colBorder
                                border.width: 1
                                Text {
                                    anchors.centerIn: parent; text: "✓"
                                    visible: index < backend.approvalCount
                                    color: root.colOk; font.pixelSize: 12; font.bold: true
                                }
                            }
                        }
                    }
                }
            }

            // ════ SECTION 3: Cast anonymous vote ════════════════════════
            Rectangle {
                Layout.fillWidth: true; Layout.fillHeight: true
                color: root.colSurface; border.color: root.colBorder; radius: 8
                ColumnLayout {
                    anchors.fill: parent; anchors.margins: 12; spacing: 8
                    Text { text: "3 · Cast anonymous vote"; color: root.colText; font.pixelSize: 14; font.bold: true }
                    Text {
                        Layout.fillWidth: true; color: root.colMuted; font.pixelSize: 11; wrapMode: Text.Wrap
                        text: "Votes with the secret entered above. The sidecar spawns run_approve_secret, a real Merkle-membership proof + proposal-bound nullifier, submitted on-chain. Non-members are rejected before any proof; a second vote with the same secret is rejected as a double vote."
                    }
                    FButton {
                        text: backend.busy ? "Proving & submitting… (~134s)" : "Cast anonymous vote"
                        enabled: !backend.busy && secretField.text.length > 0 && backend.statusReady
                        Layout.preferredHeight: 36
                        onClicked: backend.castVote(secretField.text)
                    }
                    // Busy / progress state for the long prove.
                    RowLayout {
                        visible: backend.busy
                        Layout.fillWidth: true; spacing: 10
                        BusyIndicator { running: true; implicitWidth: 20; implicitHeight: 20 }
                        ProgressBar { Layout.fillWidth: true; indeterminate: true }
                        Text { text: "generating zk proof…"; color: root.colMuted; font.pixelSize: 11 }
                    }
                    // Result: tx hash + count.
                    Rectangle {
                        visible: !!backend.lastTxHash
                        Layout.fillWidth: true
                        color: "#162033"; border.color: root.colOk; radius: 6
                        implicitHeight: resCol.implicitHeight + 16
                        ColumnLayout {
                            id: resCol
                            anchors.fill: parent; anchors.margins: 10; spacing: 4
                            Text { text: "✓ vote recorded on-chain"; color: root.colOk; font.pixelSize: 12; font.bold: true }
                            RowLayout {
                                Layout.fillWidth: true; spacing: 8
                                Text { text: "tx hash"; color: root.colMuted; font.pixelSize: 11 }
                                TextEdit {
                                    Layout.fillWidth: true; text: backend.lastTxHash
                                    color: root.colText; font.family: "monospace"; font.pixelSize: 11
                                    readOnly: true; selectByMouse: true; wrapMode: TextEdit.WrapAnywhere
                                }
                            }
                            Text {
                                color: root.colMuted; font.pixelSize: 11
                                text: "approval_count is now " + backend.approvalCount + " / " + backend.threshold
                            }
                        }
                    }
                    // Error area (e.g. "not an enrolled member" / "already voted").
                    Rectangle {
                        visible: !!backend.lastError
                        Layout.fillWidth: true
                        color: "#2a1414"; border.color: root.colErr; radius: 6
                        implicitHeight: errText.implicitHeight + 16
                        Text {
                            id: errText
                            anchors.fill: parent; anchors.margins: 10
                            text: backend.lastError; color: "#f0b0b0"; font.pixelSize: 11; wrapMode: Text.Wrap
                        }
                    }
                    Item { Layout.fillHeight: true }
                }
            }
        }

        // ── Toast ────────────────────────────────────────────────────────
        Rectangle {
            id: toast
            function flash(msg, col) { label.text = msg; color = col; opacity = 1; hideTimer.restart() }
            anchors { bottom: parent.bottom; horizontalCenter: parent.horizontalCenter; bottomMargin: 20 }
            radius: 8; opacity: 0; implicitWidth: label.implicitWidth + 24; implicitHeight: label.implicitHeight + 16
            Behavior on opacity { NumberAnimation { duration: 200 } }
            Text { id: label; anchors.centerIn: parent; color: "white"; font.pixelSize: 12 }
            Timer { id: hideTimer; interval: 5000; onTriggered: toast.opacity = 0 }
        }
    }

    // Small styled button (matches the LP-0016 template).
    component FButton: Button {
        id: btn
        contentItem: Text { text: btn.text; color: "white"; font.pixelSize: 12; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
        background: Rectangle { radius: 6; color: btn.enabled ? (btn.down ? "#6657e0" : root.colPrimary) : "#3a3f55" }
        padding: 8
    }
}
