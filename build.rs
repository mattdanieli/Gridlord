fn main() {
    // Icona e informazioni dell'eseguibile Windows
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("ProductName", "Gridlord - Playlist Organizer for FL Studio")
            .set(
                "FileDescription",
                "Gridlord - Playlist Organizer for FL Studio",
            )
            .set(
                "LegalCopyright",
                "Independent tool, not affiliated with Image-Line",
            );
        if let Err(e) = res.compile() {
            println!("cargo:warning=icona non inserita: {e}");
        }
    }
}
