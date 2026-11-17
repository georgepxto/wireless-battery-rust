//! Embute manifesto, icone e informacoes de versao no executavel.
//!
//! O manifesto e obrigatorio: sem a declaracao de compatibilidade com Windows 8+, o Windows
//! nao permite a janela filha transparente (WS_EX_LAYERED) dentro da barra de tarefas.
//!
//! `res/resources.o` ja vem compilado (COFF x86-64, gerado com windres a partir de
//! `res/app.rc`), porque o toolchain GNU do rustup nao inclui o windres e o rc.exe exige o
//! Windows SDK. O linker GNU aceita o objeto direto.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=res/resources.o");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os == "windows" && env == "gnu" {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-link-arg-bins={dir}/res/resources.o");
    } else if os == "windows" {
        println!("cargo:warning=toolchain MSVC: manifesto nao embutido; use o toolchain GNU (ver README)");
    }
}
