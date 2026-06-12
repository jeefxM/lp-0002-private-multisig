// LP-0002 anonymous-multisig Basecamp plugin (ui_qml). Modeled on the LP-0016
// ForumPlugin: a QObject implementing the Basecamp IComponent interface whose
// createWidget() returns a QQuickWidget bound to an MsigBackend context object.
#pragma once

#include <QObject>
#include <QWidget>
#include <QtPlugin>

class LogosAPI;
class MsigBackend;

class IComponent {
public:
	virtual ~IComponent() = default;
	virtual QWidget* createWidget(LogosAPI* api = nullptr) = 0;
	virtual void     destroyWidget(QWidget* widget) = 0;
};
#define IComponent_iid "com.logos.component.IComponent"
Q_DECLARE_INTERFACE(IComponent, IComponent_iid)

class MsigPlugin : public QObject, public IComponent {
	Q_OBJECT
	Q_PLUGIN_METADATA(IID IComponent_iid FILE "../manifest.json")
	Q_INTERFACES(IComponent)

public:
	explicit MsigPlugin(QObject* parent = nullptr);
	~MsigPlugin() override;

	Q_INVOKABLE void initLogos(LogosAPI* api);

	QWidget* createWidget(LogosAPI* api = nullptr) override;
	void     destroyWidget(QWidget* widget) override;

private:
	LogosAPI*    m_api     = nullptr;
	MsigBackend* m_backend = nullptr;
};
