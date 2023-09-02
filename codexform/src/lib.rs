use std::fs::read_to_string;

// Look for a generic type parameter that implements WindowSystem
fn get_ws_type(f: &syn::ItemFn) -> Option<syn::Ident> {
    let path1: syn::TraitBound = syn::parse_quote!(::trywin::WindowSystem);
    let path2: syn::TraitBound = syn::parse_quote!(trywin::WindowSystem);
    for gen in &f.sig.generics.params {
        if let syn::GenericParam::Type(t) = gen {
            for bound in &t.bounds {
                if let syn::TypeParamBound::Trait(trait_) = bound {
                    if trait_ == &path1 || trait_ == &path2 {
                        return Some(t.ident.clone());
                    }
                }
            }
        }
    }
    if let Some(where_clause) = &f.sig.generics.where_clause {
        for pred in &where_clause.predicates {
            if let syn::WherePredicate::Type(t) = pred {
                if let syn::Type::Path(path) = &t.bounded_ty {
                    if path.qself.is_some() {
                        continue;
                    }
                    if let Some(ident) = path.path.get_ident() {
                        for bound in &t.bounds {
                            if let syn::TypeParamBound::Trait(trait_) = bound {
                                if trait_ == &path1 || trait_ == &path2 {
                                    return Some(ident.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

// Look for an argument of type WS (references allowed)
fn get_ws_arg(f: &syn::ItemFn, ws: &syn::Ident) -> Option<syn::Ident> {
    for arg in &f.sig.inputs {
        if let syn::FnArg::Typed(t) = arg {
            let mut ty = &*t.ty;
            while let syn::Type::Reference(r) = ty {
                ty = &*r.elem;
            }
            if let syn::Type::Path(path) = ty {
                if path.qself.is_some() {
                    continue;
                }
                if let Some(ident) = path.path.get_ident() {
                    if ident == ws {
                        if let syn::Pat::Ident(pat) = &*t.pat {
                            return Some(pat.ident.clone());
                        }
                    }
                }
            }
        }
    }
    None
}

struct WSFunction<'a> {
    _f: &'a mut syn::ItemFn,
    ws_type: syn::Ident, // Generic parameter which implements WindowSystem
    ws_arg: syn::Ident,  // Argument of type WS (references allowed)
}

impl<'a> WSFunction<'a> {
    fn new(f: &'a mut syn::ItemFn) -> Option<Self> {
        let ws_type = get_ws_type(f)?;
        let ws_arg = get_ws_arg(f, &ws_type)?;
        Some(Self {
            _f: f,
            ws_type,
            ws_arg,
        })
    }
}

// TODO: error type
pub fn scan_file() -> Result<(), Box<dyn std::error::Error>> {
    let mut ast = syn::parse_file(&read_to_string("../trywin/src/main.rs")?)?;
    for item in ast.items.iter_mut() {
        if let syn::Item::Fn(f) = item {
            let Some(ws_fn) = WSFunction::new(f) else {
                continue;
            };
            println!("{} {}", ws_fn.ws_type, ws_fn.ws_arg);
        }
    }
    Ok(())
}
