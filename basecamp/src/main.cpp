// Standalone preview app — loads the QML UI without Basecamp (same backend the
// Basecamp plugin uses). Build: cmake -B build && cmake --build build
// Run: DISPLAY=:1 QT_QPA_PLATFORM=xcb MSIG_SIDECAR_URL=http://127.0.0.1:8799 ./build/MsigApp
#include "MsigBackend.h"
#include "MsigPlugin.h"

#include <QApplication>
#include <QQmlContext>
#include <QQmlEngine>
#include <QQuickWidget>
#include <QUrl>
#include <cstdlib>

int main(int argc, char** argv) {
	QApplication app(argc, argv);
	app.setOrganizationName("logos-co");
	app.setApplicationName("msig-lp0002");

	MsigBackend backend(nullptr);

	QQuickWidget view;
	view.engine()->rootContext()->setContextProperty("backend", &backend);
	view.setResizeMode(QQuickWidget::SizeRootObjectToView);
	view.resize(820, 640);

	const char* qmlPath = std::getenv("QML_PATH");
	if (qmlPath)
		view.setSource(QUrl::fromLocalFile(QString::fromUtf8(qmlPath) + "/Main.qml"));
	else
		view.setSource(QUrl("qrc:/qml/Main.qml"));

	view.setWindowTitle("Private Multisig (LP-0002)");
	view.show();
	return app.exec();
}
