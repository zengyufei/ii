#[cfg(windows)]
fn embed_windows_resources() {
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("../logo.ico");
    resource.set("FileDescription", "ii file transfer GUI");
    resource.set("ProductName", "ii");
    resource.compile().expect("compile Windows GUI resources");
}

#[cfg(not(windows))]
fn embed_windows_resources() {}

fn main() {
    println!("cargo:rerun-if-changed=../logo.ico");
    slint_build::compile("ui/main.slint").expect("compile Slint UI");
    embed_windows_resources();
}
