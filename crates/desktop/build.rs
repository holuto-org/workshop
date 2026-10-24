use std::{env, error::Error};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed=assets/icon.ico");
    println!("cargo::rerun-if-env-changed=SENTRY_DSN");
    println!("cargo::rerun-if-env-changed=SENTRY_ENVIRONMENT");
    if env::var("PROFILE").as_deref() == Ok("debug") {
        println!("cargo::rustc-env=SLINT_EMIT_DEBUG_INFO=1");
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .set("ProductName", "Holuto Workshop")
            .set("FileDescription", "Holuto Workshop · Posnet Temo Online")
            .compile()?;
    }
    Ok(())
}
