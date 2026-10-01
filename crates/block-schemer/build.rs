//! Gives the Windows executable its icon, which Explorer and shortcuts show.
//! The running window's icon is set separately, in `main`.

fn main() {
    println!("cargo::rerun-if-changed=assets/icons/block-schemer.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/icons/block-schemer.ico")
            .compile()
            .expect("the Windows icon resource compiles");
    }
}
