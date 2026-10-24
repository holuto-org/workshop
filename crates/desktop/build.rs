use std::{env, error::Error};

fn main() -> Result<(), Box<dyn Error>> {
    slint_build::compile_with_config(
        "ui/app.slint",
        slint_build::CompilerConfiguration::new().with_style("fluent".into()),
    )?;
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .set("ProductName", "Holuto Fiscal")
            .set("FileDescription", "Holuto Fiscal · Posnet Temo Online")
            .compile()?;
    }
    Ok(())
}
