fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/ekaza-cam.ico");
        if let Err(e) = res.compile() {
            eprintln!("Aviso: falha ao compilar recurso Windows: {e}");
        }
    }
}
