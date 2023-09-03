use std::fs::read_to_string;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ast = syn::parse_file(&read_to_string("../trywin/src/main.rs")?)?;
    codexform::get_functions(&ast)?;
    Ok(())
}
