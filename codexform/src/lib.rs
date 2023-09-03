#![allow(dead_code)]

use std::collections::HashMap;

fn get_type_ident(ty: &syn::Type) -> Option<&syn::Ident> {
    if let syn::Type::Path(path) = ty {
        if path.qself.is_none() {
            if let Some(ident) = path.path.get_ident() {
                return Some(ident);
            }
        }
    }
    None
}

fn get_expr_ident(expr: &syn::Expr) -> Option<&syn::Ident> {
    if let syn::Expr::Path(path) = expr {
        if path.qself.is_none() {
            if let Some(ident) = path.path.get_ident() {
                return Some(ident);
            }
        }
    }
    None
}

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
                if let Some(ident) = get_type_ident(&t.bounded_ty) {
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
            if let Some(ident) = get_type_ident(ty) {
                if ident == ws {
                    if let syn::Pat::Ident(pat) = &*t.pat {
                        return Some(pat.ident.clone());
                    }
                }
            }
        }
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MethodType {
    Create,
    Attr,
    Event,
}

impl MethodType {
    fn from_ident(ident: &syn::Ident) -> Self {
        let ident = ident.to_string();
        if ident.starts_with("new_") {
            Self::Create
        } else if ident.starts_with("on_") {
            Self::Event
        } else {
            Self::Attr
        }
    }
}

// .a()
struct Method<'a> {
    dot: &'a syn::token::Dot,
    ident: &'a syn::Ident,
    method_type: MethodType,
    paren: &'a syn::token::Paren,
    args: &'a syn::punctuated::Punctuated<syn::Expr, syn::token::Comma>,
}

// x.a().b().c()
struct MethodChain<'a> {
    expr: &'a syn::Expr,
    ident: &'a syn::Ident,
    methods: Vec<Method<'a>>,
}

impl<'a> MethodChain<'a> {
    fn new(expr: &'a syn::Expr) -> Option<Self> {
        let mut methods = Vec::new();
        let mut expr = expr;
        while let syn::Expr::MethodCall(method_call) = expr {
            if !method_call.attrs.is_empty() || method_call.turbofish.is_some() {
                break;
            }
            methods.push(Method {
                dot: &method_call.dot_token,
                ident: &method_call.method,
                method_type: MethodType::from_ident(&method_call.method),
                paren: &method_call.paren_token,
                args: &method_call.args,
            });
            expr = &method_call.receiver;
        }
        if let Some(ident) = get_expr_ident(expr) {
            methods.reverse();
            return Some(Self {
                expr,
                ident,
                methods,
            });
        }
        None
    }
}

// let x = parent.create().a().b().c();
struct Definition<'a> {
    statement: &'a syn::Stmt,
    local: &'a syn::Local,
    ident: &'a syn::Ident,
    parent: &'a syn::Ident,
    create: Method<'a>,
    methods: Vec<Method<'a>>,
}

// let x = x.a().b().c();
struct Redefinition<'a> {
    statement: &'a syn::Stmt,
    local: &'a syn::Local,
    ident: &'a syn::Ident,
    methods: Vec<Method<'a>>,
}

// x.a().b().c();
struct Use<'a> {
    statement: &'a syn::Stmt,
    expr: &'a syn::Expr,
    ident: &'a syn::Ident,
    methods: Vec<Method<'a>>,
}

enum Item<'a> {
    Definition(Definition<'a>),
    Redefinition(Redefinition<'a>),
    Use(Use<'a>),
}

impl<'a> Item<'a> {
    fn ident(&self) -> &'a syn::Ident {
        match self {
            Self::Definition(item) => item.ident,
            Self::Redefinition(item) => item.ident,
            Self::Use(item) => item.ident,
        }
    }
}

impl<'a> Item<'a> {
    fn new(statement: &'a syn::Stmt) -> Option<Self> {
        match statement {
            syn::Stmt::Local(local) => {
                let syn::Pat::Ident(ident) = &local.pat else {
                    return None;
                };
                if !local.attrs.is_empty() || !ident.attrs.is_empty() || ident.subpat.is_some() {
                    return None;
                }
                let ident = &ident.ident;
                let Some(init) = &local.init else {
                    return None;
                };
                if init.diverge.is_some() {
                    return None;
                }
                let mut chain = MethodChain::new(&init.expr)?;
                if chain.methods.is_empty() {
                    return None;
                }
                if chain.ident != ident && chain.methods[0].method_type == MethodType::Create {
                    let create = chain.methods.remove(0);
                    return Some(Self::Definition(Definition {
                        statement,
                        local,
                        ident,
                        parent: chain.ident,
                        create,
                        methods: chain.methods,
                    }));
                }
                if chain.ident == ident && chain.methods[0].method_type != MethodType::Create {
                    return Some(Self::Redefinition(Redefinition {
                        statement,
                        local,
                        ident,
                        methods: chain.methods,
                    }));
                }
                None
            }
            syn::Stmt::Expr(expr, _semi) => {
                if let Some(chain) = MethodChain::new(expr) {
                    if !chain.methods.is_empty() {
                        return Some(Self::Use(Use {
                            statement,
                            expr,
                            ident: chain.ident,
                            methods: chain.methods,
                        }));
                    }
                }
                None
            }
            _ => None,
        }
    }
}

pub struct WSFunction<'a> {
    f: &'a syn::ItemFn,
    ws_type: syn::Ident, // Type which implements WindowSystem
    ws_arg: syn::Ident,  // Argument of type WS (references allowed)
    items: HashMap<&'a syn::Ident, Vec<Item<'a>>>,
}

impl<'a> WSFunction<'a> {
    fn new(f: &'a syn::ItemFn) -> Option<Self> {
        let ws_type = get_ws_type(f)?;
        let ws_arg = get_ws_arg(f, &ws_type)?;
        let mut items = HashMap::<&syn::Ident, Vec<_>>::new();
        for stmt in &f.block.stmts {
            if let Some(item) = Item::new(stmt) {
                items.entry(item.ident()).or_default().push(item);
            }
        }
        Some(Self {
            f,
            ws_type,
            ws_arg,
            items,
        })
    }
}

// TODO: error type
pub fn get_functions(ast: &syn::File) -> Result<Vec<WSFunction>, Box<dyn std::error::Error>> {
    let mut functions = Vec::new();
    for item in ast.items.iter() {
        if let syn::Item::Fn(f) = item {
            if let Some(ws_fn) = WSFunction::new(f) {
                println!("{} {}", ws_fn.ws_type, ws_fn.ws_arg);
                functions.push(ws_fn);
            }
        }
    }
    Ok(functions)
}
