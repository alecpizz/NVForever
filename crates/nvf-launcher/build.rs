//! The launcher's icon in the Windows executable (Explorer, shortcuts); the window sets the
//! same picture itself (main.rs). Common Controls 6 for the folder picker, as nvforever's
//! build.rs embeds it.
#[cfg(all(windows, feature = "desktop"))]
const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
    </dependentAssembly>
  </dependency>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-changed=assets/nvf-launcher.ico");
    #[cfg(all(windows, feature = "desktop"))]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/nvf-launcher.ico");
        res.set_manifest(MANIFEST);
        res.compile().expect("embedding the launcher icon (needs the Windows SDK's rc.exe)");
    }
}
