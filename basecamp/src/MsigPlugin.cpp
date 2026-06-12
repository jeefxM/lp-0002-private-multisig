// LP-0002 anonymous-multisig Basecamp plugin. Modeled on ForumPlugin.cpp.
#include "MsigPlugin.h"
#include "MsigBackend.h"

#include <QQmlContext>
#include <QQmlEngine>
#include <QQuickWidget>
#include <QUrl>
#include <cstdlib>

MsigPlugin::MsigPlugin(QObject* parent) : QObject(parent) {}
MsigPlugin::~MsigPlugin() = default;

void MsigPlugin::initLogos(LogosAPI* api) {
	m_api = api;
}

QWidget* MsigPlugin::createWidget(LogosAPI* api) {
	if (api) m_api = api;
	if (!m_backend)
		m_backend = new MsigBackend(m_api, this);
	auto* view = new QQuickWidget();
	view->engine()->rootContext()->setContextProperty("backend", m_backend);
	view->setResizeMode(QQuickWidget::SizeRootObjectToView);
	const char* qmlPath = std::getenv("QML_PATH");
	if (qmlPath) {
		view->setSource(QUrl::fromLocalFile(QString::fromUtf8(qmlPath) + "/Main.qml"));
	} else {
		// Qt does not auto-register embedded resources in dynamically loaded plugins.
		Q_INIT_RESOURCE(msig_qml); // name must match qt_add_resources() in CMakeLists.txt
		view->setSource(QUrl("qrc:/qml/Main.qml"));
	}
	return view;
}

void MsigPlugin::destroyWidget(QWidget* widget) {
	delete m_backend;
	m_backend = nullptr;
	delete widget;
}
