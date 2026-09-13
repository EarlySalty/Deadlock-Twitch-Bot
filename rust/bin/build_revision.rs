//! Gemeinsame Herkunftsmarke für die drei ausgelieferten Twitch-Binaries.
use std::{fs, path::PathBuf, process::Command};

fn git(args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .output()
        .expect("Git für Build-Herkunft");
    assert!(
        output.status.success(),
        "Git-Herkunft konnte nicht gelesen werden"
    );
    String::from_utf8(output.stdout)
        .expect("Git-Ausgabe als UTF-8")
        .trim()
        .to_owned()
}

fn main() {
    // Absichtlich nicht vorhanden: bei JEDEM Cargo-Aufruf neu auswerten, auch
    // nach Commit/Branch-Wechsel oder Änderungen außerhalb dieses Bin-Pakets.
    println!("cargo:rerun-if-changed=.build-revision-always-check");
    let revision = git(&["rev-parse", "HEAD"]);
    assert!(revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit()));
    let dirty = !git(&["status", "--porcelain", "--untracked-files=no"]).is_empty();
    let revision = format!("{revision}{}", if dirty { "-dirty" } else { "" });
    let source = format!(
        "#[used]\n#[unsafe(link_section = \".twitch_build\")]\n\
         static BUILD_REVISION: [u8; {}] = *b\"{}\\0\";\n\
         fn print_build_revision() -> bool {{\n\
             if std::env::args().nth(1).as_deref() == Some(\"--build-revision\") {{\n\
                 println!(\"{{}}\", std::str::from_utf8(&BUILD_REVISION[..BUILD_REVISION.len()-1]).unwrap());\n\
                 true\n\
             }} else {{ false }}\n\
         }}\n",
        revision.len() + 1, revision
    );
    // OUT_DIR ist ausschließlich Cargos Build-Ausgabeverzeichnis, keine Dienstkonfiguration.
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    fs::write(out.join("build_revision.rs"), source).expect("Build-Herkunft schreiben");
}
