use shadow_rs::ShadowBuilder;

fn main() {
    ShadowBuilder::builder().build().unwrap();
    #[cfg(windows)]
    {
        let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let icon_path = std::path::Path::new(&manifest_dir).join("assets").join("icon.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon(icon_path.to_str().unwrap());
        res.compile().unwrap();
    }
}
