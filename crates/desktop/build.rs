use std::{env, error::Error};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed=assets/icon.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .set("ProductName", "Holuto Fiscal")
            .set("FileDescription", "Holuto Fiscal · Posnet Temo Online")
            .compile()?;
    }
    Ok(())
}
