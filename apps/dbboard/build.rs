//! Windows resource embedding (ADR-0032).
//!
//! On the MSVC target this stamps the release binary with an icon and the
//! standard version/product metadata so a handed-off `dbboard.exe` looks
//! like a real application in Explorer, the taskbar, and the "Details"
//! tab — instead of the default blank Rust icon. It is a no-op on every
//! other platform, so `cargo build` on macOS/Linux is unaffected.
//!
//! Version fields default to `CARGO_PKG_VERSION`; only the human-facing
//! strings are set explicitly here.

fn main() {
    // Only the Windows resource compiler understands `.rc`/`.ico`; on any
    // other host there is nothing to embed.
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/dbboard.ico");
        res.set("ProductName", "dbboard");
        res.set(
            "FileDescription",
            "dbboard — desktop client for serverless databases",
        );
        res.set("CompanyName", "meta-taro");
        res.set("LegalCopyright", "MIT License. See LICENSE.");
        res.set("OriginalFilename", "dbboard.exe");
        // Rebuild the embedded resource when the icon changes.
        println!("cargo:rerun-if-changed=assets/dbboard.ico");
        res.compile()
            .expect("failed to embed Windows resources (icon + version metadata)");
    }
}
