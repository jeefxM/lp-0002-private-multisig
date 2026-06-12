#include "MsigBackend.h"

#include <QByteArray>
#include <QCryptographicHash>
#include <QJsonDocument>
#include <QJsonObject>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QProcessEnvironment>
#include <QSettings>
#include <QUrl>

namespace {
// LEAF_DOMAIN = b"/lp0002/leaf/\x00" — 14 bytes (13 ASCII + one trailing NUL).
// A bare const char* would truncate at the NUL, so build it explicitly.
QByteArray leafDomain() {
	QByteArray d("/lp0002/leaf/"); // 13 bytes
	d.append('\0');                // 14th byte: the explicit trailing NUL
	return d;
}
} // namespace

MsigBackend::MsigBackend(LogosAPI* /*api*/, QObject* parent) : QObject(parent) {
	m_net = new QNetworkAccessManager(this);
	// The /approve request runs the full ~134s prove + a block wait. Disable the
	// per-reply transfer timeout so the socket is not killed mid-prove (0 = off).
	m_net->setTransferTimeout(0);

	QSettings s("logos-co", "msig-lp0002");
	const QString envUrl = QProcessEnvironment::systemEnvironment().value(
		"MSIG_SIDECAR_URL", "http://127.0.0.1:8799");
	m_sidecarUrl = s.value("sidecarUrl", envUrl).toString();
}

MsigBackend::~MsigBackend() = default;

void MsigBackend::setSidecarUrl(const QString& v) {
	if (m_sidecarUrl == v) return;
	m_sidecarUrl = v;
	QSettings("logos-co", "msig-lp0002").setValue("sidecarUrl", v);
	emit sidecarUrlChanged();
}

void MsigBackend::setBusy(bool b) {
	if (m_busy == b) return;
	m_busy = b;
	emit busyChanged();
}

void MsigBackend::setError(const QString& /*op*/, const QString& msg) {
	m_lastError = msg;
	emit lastErrorChanged();
}

// ── deriveLeaf: local, pure (the secret stays on this machine) ───────────────

QString MsigBackend::deriveLeaf(const QString& secretHex) {
	QString h = secretHex.trimmed();
	if (h.startsWith("0x") || h.startsWith("0X")) h = h.mid(2);
	const QByteArray secret = QByteArray::fromHex(h.toUtf8());
	if (secret.size() != 32) {
		setError("derive", QStringLiteral("secret must be exactly 32 bytes (64 hex chars); got %1 bytes").arg(secret.size()));
		m_lastLeaf.clear();
		emit lastLeafChanged();
		emit operationError("derive", m_lastError);
		return QString();
	}
	QByteArray buf = leafDomain();
	buf.append(secret);
	const QByteArray digest = QCryptographicHash::hash(buf, QCryptographicHash::Sha256);
	m_lastLeaf = QString::fromLatin1(digest.toHex());
	emit lastLeafChanged();
	if (!m_lastError.isEmpty()) { m_lastError.clear(); emit lastErrorChanged(); }
	emit operationSuccess("derive", m_lastLeaf.left(16) + "…");
	return m_lastLeaf;
}

// ── applyStatus: populate the status properties from a parsed JSON object ─────

void MsigBackend::applyStatus(const QVariantMap& obj) {
	m_statusReady = obj.value("ready").toBool();
	if (obj.contains("threshold")) m_threshold = obj.value("threshold").toInt();
	if (obj.contains("proposal_id")) m_proposalId = obj.value("proposal_id").toString();
	if (m_statusReady) {
		m_memberRoot = obj.value("member_root").toString();
		m_approvalCount = obj.value("approval_count").toInt();
	}
	emit statusChanged();
}

// ── getStatus: GET <sidecar>/status ──────────────────────────────────────────

void MsigBackend::getStatus() {
	if (m_busy) { emit operationError("status", "another operation is in progress"); return; }
	setBusy(true);
	QNetworkRequest req{QUrl(m_sidecarUrl + "/status")};
	QNetworkReply* reply = m_net->get(req);
	connect(reply, &QNetworkReply::finished, this, [this, reply]() {
		const QByteArray body = reply->readAll();
		const QNetworkReply::NetworkError err = reply->error();
		reply->deleteLater();
		setBusy(false);
		if (err != QNetworkReply::NoError && body.isEmpty()) {
			setError("status", "sidecar unreachable: " + reply->errorString());
			emit operationError("status", m_lastError);
			return;
		}
		const QJsonObject o = QJsonDocument::fromJson(body).object();
		if (o.contains("error")) {
			setError("status", o.value("error").toString());
			emit operationError("status", m_lastError);
			return;
		}
		applyStatus(o.toVariantMap());
		if (!m_lastError.isEmpty()) { m_lastError.clear(); emit lastErrorChanged(); }
		emit operationSuccess("status", m_statusReady
			? QStringLiteral("count %1/%2").arg(m_approvalCount).arg(m_threshold)
			: QStringLiteral("proposal not created yet"));
	});
}

// ── castVote: POST <sidecar>/approve {secret_hex} ────────────────────────────

void MsigBackend::castVote(const QString& secretHex) {
	if (m_busy) { emit operationError("vote", "another operation is in progress"); return; }
	QString h = secretHex.trimmed();
	if (h.startsWith("0x") || h.startsWith("0X")) h = h.mid(2);
	if (QByteArray::fromHex(h.toUtf8()).size() != 32) {
		setError("vote", "secret must be exactly 32 bytes (64 hex chars) before voting");
		emit operationError("vote", m_lastError);
		return;
	}
	setBusy(true);
	QNetworkRequest req{QUrl(m_sidecarUrl + "/approve")};
	req.setHeader(QNetworkRequest::ContentTypeHeader, "application/json");
	const QJsonObject payload{{"secret_hex", h}};
	QNetworkReply* reply = m_net->post(req, QJsonDocument(payload).toJson(QJsonDocument::Compact));
	connect(reply, &QNetworkReply::finished, this, [this, reply]() {
		const QByteArray body = reply->readAll();
		const QNetworkReply::NetworkError err = reply->error();
		reply->deleteLater();
		setBusy(false);
		QJsonObject o = QJsonDocument::fromJson(body).object();
		if (err != QNetworkReply::NoError && body.isEmpty()) {
			setError("vote", "sidecar unreachable: " + reply->errorString());
			emit operationError("vote", m_lastError);
			return;
		}
		const bool ok = o.value("success").toBool();
		if (!ok) {
			QString e = o.value("error").toString();
			if (e.isEmpty()) e = "vote rejected (no detail from sidecar)";
			setError("vote", e);
			emit operationError("vote", m_lastError);
			return;
		}
		const QString tx = o.value("tx_hash").toString();
		if (!tx.isEmpty() && tx != m_lastTxHash) {
			m_lastTxHash = tx;
			emit lastTxHashChanged();
		}
		// The sidecar re-reads status (after a block) and returns the fresh count.
		if (o.contains("approval_count")) {
			m_approvalCount = o.value("approval_count").toInt();
			m_statusReady = true;
			emit statusChanged();
		}
		if (!m_lastError.isEmpty()) { m_lastError.clear(); emit lastErrorChanged(); }
		emit operationSuccess("vote", QStringLiteral("tx %1 · count %2/%3")
			.arg(tx.left(12)).arg(m_approvalCount).arg(m_threshold));
	});
}
