use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("io.github.oowl.kowly").qml_files(["qml/Main.qml"]),
    )
    .files(["src/bridge.rs"])
    .build();
}
