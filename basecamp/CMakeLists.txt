cmake_minimum_required(VERSION 3.16)
project(MsigLp0002 VERSION 1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)

# Network is added vs the LP-0016 template: this backend talks to the sidecar
# over HTTP (persistent localhost server), not via a one-shot QProcess CLI.
find_package(Qt6 REQUIRED COMPONENTS Core Gui Widgets Qml Quick QuickWidgets Concurrent Network)

# ── Plugin (loaded by Basecamp) ────────────────────────────────────────────
add_library(MsigPlugin SHARED
    src/MsigBackend.cpp
    src/MsigPlugin.cpp
)

# Resource name "msig_qml" must match Q_INIT_RESOURCE(msig_qml) in MsigPlugin.cpp
qt_add_resources(MsigPlugin "msig_qml"
    PREFIX "/"
    FILES qml/Main.qml
)

target_link_libraries(MsigPlugin PRIVATE
    Qt6::Core Qt6::Gui Qt6::Widgets Qt6::Qml Qt6::Quick Qt6::QuickWidgets Qt6::Concurrent Qt6::Network
)

# Output name must match manifest.json "main" value (lib<name>.so == libmsig_plugin.so)
set_target_properties(MsigPlugin PROPERTIES OUTPUT_NAME "msig_plugin")

# ── Standalone preview app ─────────────────────────────────────────────────
add_executable(MsigApp
    src/main.cpp
    src/MsigBackend.cpp
    src/MsigPlugin.cpp
)

# Same resource name as the plugin so Q_INIT_RESOURCE resolves in both targets.
qt_add_resources(MsigApp "msig_qml"
    PREFIX "/"
    FILES qml/Main.qml
)

target_link_libraries(MsigApp PRIVATE
    Qt6::Core Qt6::Gui Qt6::Widgets Qt6::Qml Qt6::Quick Qt6::QuickWidgets Qt6::Concurrent Qt6::Network
)
