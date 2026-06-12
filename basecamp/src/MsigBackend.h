// LP-0002 anonymous-multisig Basecamp module backend.
//
// Drives a localhost-only Node sidecar (basecamp/msig-sidecar.mjs) over HTTP via
// QNetworkAccessManager. The sidecar in turn shells out to the proven Rust
// runners (run_read_status for /status, run_approve_secret for /approve) so the
// sandboxed QML/plugin never does the proving or the on-chain submit itself —
// the real ~134s Groth/STARK prove + tx happen in the spawned runner.
//
// deriveLeaf() is the ONE thing computed locally in C++ (QCryptographicHash):
// the member leaf SHA256("/lp0002/leaf/\x00" || secret). The 32-byte secret is
// hashed in-process and only the leaf hash leaves this widget — the secret stays
// on the local machine.
#pragma once

#include <QObject>
#include <QString>
#include <QVariantMap>

class LogosAPI;
class QNetworkAccessManager;

class MsigBackend : public QObject {
	Q_OBJECT

	// Busy across the ~134s prove; drives the QML progress / busy state.
	Q_PROPERTY(bool busy READ busy NOTIFY busyChanged)
	Q_PROPERTY(QString lastError READ lastError NOTIFY lastErrorChanged)
	Q_PROPERTY(QString sidecarUrl READ sidecarUrl WRITE setSidecarUrl NOTIFY sidecarUrlChanged)

	// Proposal status (populated by getStatus() -> sidecar /status).
	Q_PROPERTY(bool statusReady READ statusReady NOTIFY statusChanged)
	Q_PROPERTY(QString proposalId READ proposalId NOTIFY statusChanged)
	Q_PROPERTY(QString memberRoot READ memberRoot NOTIFY statusChanged)
	Q_PROPERTY(int approvalCount READ approvalCount NOTIFY statusChanged)
	Q_PROPERTY(int threshold READ threshold NOTIFY statusChanged)

	// Derive-my-leaf result (local, pure).
	Q_PROPERTY(QString lastLeaf READ lastLeaf NOTIFY lastLeafChanged)

	// Cast-vote result.
	Q_PROPERTY(QString lastTxHash READ lastTxHash NOTIFY lastTxHashChanged)

public:
	explicit MsigBackend(LogosAPI* api = nullptr, QObject* parent = nullptr);
	~MsigBackend() override;

	bool busy() const { return m_busy; }
	QString lastError() const { return m_lastError; }
	QString sidecarUrl() const { return m_sidecarUrl; }

	bool statusReady() const { return m_statusReady; }
	QString proposalId() const { return m_proposalId; }
	QString memberRoot() const { return m_memberRoot; }
	int approvalCount() const { return m_approvalCount; }
	int threshold() const { return m_threshold; }

	QString lastLeaf() const { return m_lastLeaf; }
	QString lastTxHash() const { return m_lastTxHash; }

	Q_INVOKABLE void setSidecarUrl(const QString& v);

	// Local, pure: leaf = SHA256("/lp0002/leaf/\x00" || secret). Returns hex, or
	// empty string + sets lastError on a malformed secret. The secret never
	// leaves the process via this call.
	Q_INVOKABLE QString deriveLeaf(const QString& secretHex);

	// GET <sidecar>/status -> proposal_id, member_root, approval_count, threshold.
	Q_INVOKABLE void getStatus();

	// POST <sidecar>/approve {secret_hex} -> spawns run_approve_secret; on success
	// returns the real tx_hash and the updated approval_count (after a block wait,
	// handled sidecar-side). Surfaces "not an enrolled member" / "already voted".
	Q_INVOKABLE void castVote(const QString& secretHex);

signals:
	void busyChanged();
	void lastErrorChanged();
	void sidecarUrlChanged();
	void statusChanged();
	void lastLeafChanged();
	void lastTxHashChanged();
	void operationSuccess(const QString& operation, const QString& detail);
	void operationError(const QString& operation, const QString& error);

private:
	void setBusy(bool b);
	void setError(const QString& op, const QString& msg);
	void applyStatus(const QVariantMap& obj);

	QNetworkAccessManager* m_net = nullptr;
	QString m_sidecarUrl;
	QString m_lastError;
	bool m_busy = false;

	bool m_statusReady = false;
	QString m_proposalId;
	QString m_memberRoot;
	int m_approvalCount = 0;
	int m_threshold = 0;

	QString m_lastLeaf;
	QString m_lastTxHash;
};
