fn main() {
    println!("cargo:rerun-if-changed=../../assets/branding/oxy.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set_language(0x0416)
            .set("ProductName", "OXY Engine")
            .set("FileDescription", "OXY Engine — Editor de jogos")
            .set("InternalName", "oxy_editor")
            .set("OriginalFilename", "OXY Engine.exe")
            .set("CompanyName", "Ratcicle");
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu") {
            // windres reparses absolute -I paths containing spaces when invoking cpp.
            // Compile relative filenames in OUT_DIR instead (the repository has a space).
            let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
            std::fs::copy("../../assets/branding/oxy.ico", output.join("oxy.ico")).unwrap();
            resource.set_icon("oxy.ico");
            resource
                .write_resource_file(output.join("resource.rc"))
                .unwrap();
            let status = std::process::Command::new(
                std::env::var("WINDRES").unwrap_or_else(|_| "windres".into()),
            )
            .current_dir(&output)
            .args(["-i", "resource.rc", "-o", "resource.o", "-O", "coff"])
            .status()
            .expect("Compilador de recursos windres indisponível");
            assert!(status.success(), "Falha ao compilar recursos Windows");
            println!(
                "cargo:rustc-link-arg={}",
                output.join("resource.o").display()
            );
        } else {
            resource.set_icon("../../assets/branding/oxy.ico");
            resource
                .compile()
                .expect("Não foi possível compilar o ícone/metadados da OXY Engine");
        }
    }
}
