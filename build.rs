fn main() {
    slint_build::compile("ui/app.slint").expect("Slint build failed");

    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/app_icon.ico");
        let _ = res.compile();
    }
}
