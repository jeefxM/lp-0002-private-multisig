# LP-0002 Basecamp module

The anonymous M-of-N multisig Basecamp (`ui_qml`) GUI for this submission: it casts a real
anonymous vote through the same client path as the CLI runners (a localhost sidecar drives the
already-proven LEZ runners).

`module.json` / `metadata.json` describe the module — type `ui_qml`, view `qml/Main.qml`, backend
plugin `msig_plugin`, variants `darwin-arm64` + `linux-amd64` + `linux-arm64`. The QML view is in
`qml/Main.qml`.

**Full source, the Nix flake that builds the portable plugin, and the localhost sidecar are maintained at:**
https://github.com/jeefxM/logos-lp0002-msig-module

**Prebuilt, signed, multi-variant download (`.lgx`):**
https://github.com/jeefxM/logos-lp0002-msig-module/releases/tag/v0.1.0 — install via Basecamp →
Package Manager → *Install from file*; verify with `lgx verify`.
