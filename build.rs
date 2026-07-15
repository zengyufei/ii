#[cfg(windows)]
fn main() {
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("logo.ico");
    resource.set("FileDescription", "ii file transfer CLI");
    resource.set("ProductName", "ii");
    resource.compile().expect("compile Windows resources");
}

#[cfg(not(windows))]
fn main() {}
