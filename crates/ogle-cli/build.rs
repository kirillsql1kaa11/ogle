use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=OGLE_EMBED_SHELL");
    println!("cargo:rustc-check-cfg=cfg(ogle_embedded_shell)");

    if std::env::var("OGLE_EMBED_SHELL").as_deref() != Ok("1") {
        return;
    }

    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let release = out
        .ancestors()
        .nth(3)
        .expect("каталог release")
        .to_path_buf();
    let dll = release.join("ogle_shell.dll");
    println!("cargo:rerun-if-changed={}", dll.display());

    let bytes = std::fs::read(&dll).unwrap_or_else(|error| {
        panic!(
            "OGLE_EMBED_SHELL=1, но {} не найден ({error}). Сначала: cargo build --release -p ogle-shell",
            dll.display()
        )
    });
    std::fs::write(out.join("ogle_shell.dll"), bytes)
        .expect("не удалось скопировать ogle_shell.dll в OUT_DIR");
}
