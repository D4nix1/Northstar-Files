fn main() {
    println!("cargo:rerun-if-changed=assets/northstar.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resources = winres::WindowsResource::new();
        resources
            .set_icon("assets/northstar.ico")
            .set("ProductName", "Northstar Files")
            .set(
                "FileDescription",
                "Northstar Files - experimental file manager",
            )
            .set("CompanyName", "Northstar Open Source")
            .compile()
            .expect("could not embed the Northstar application icon; install the Windows SDK");
    }
}
