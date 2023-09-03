use std::fs::read_to_string;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ast = syn::parse_file(&read_to_string("../trywin/src/main.rs")?)?;
    let f = codexform::get_functions(&ast)?;
    let f = f
        .iter()
        .map(codexform::serializable::Function::from)
        .collect::<Vec<_>>();
    println!("{}", serde_json::to_string_pretty(&f)?);
    Ok(())
}
