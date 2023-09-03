use quote::quote;
use serde::{Deserialize, Serialize};
use syn::spanned::Spanned;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl From<proc_macro2::LineColumn> for Position {
    fn from(lc: proc_macro2::LineColumn) -> Self {
        Self {
            line: lc.line,
            column: lc.column,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Span {
    pub start: Position,
    pub end: Position,
}

impl From<&proc_macro2::Span> for Span {
    fn from(span: &proc_macro2::Span) -> Self {
        Self {
            start: span.start().into(),
            end: span.end().into(),
        }
    }
}

impl From<proc_macro2::Span> for Span {
    fn from(span: proc_macro2::Span) -> Self {
        Self {
            start: span.start().into(),
            end: span.end().into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Arg {
    pub span: Span,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Method {
    pub span: Span,
    pub name: String,
    pub args: Vec<Arg>,
}

impl<'a> From<&crate::Method<'a>> for Method {
    fn from(m: &crate::Method) -> Self {
        Self {
            span: Span {
                start: m.dot.span.start().into(),
                end: m.paren.span.close().end().into(),
            },
            name: m.ident.to_string(),
            args: m
                .args
                .iter()
                .map(|arg| Arg {
                    span: arg.span().into(),
                    value: quote!(arg).to_string(),
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Definition {
    pub span: Span,
    pub name: String,
    pub parent: String,
    pub create: Method,
    pub methods: Vec<Method>,
}

impl<'a> From<&crate::Definition<'a>> for Definition {
    fn from(d: &crate::Definition<'a>) -> Self {
        Self {
            span: d.statement.span().into(),
            name: d.ident.to_string(),
            parent: d.parent.to_string(),
            create: (&d.create).into(),
            methods: d.methods.iter().map(Method::from).collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Redefinition {
    pub span: Span,
    pub name: String,
    pub methods: Vec<Method>,
}

impl<'a> From<&crate::Redefinition<'a>> for Redefinition {
    fn from(r: &crate::Redefinition<'a>) -> Self {
        Self {
            span: r.statement.span().into(),
            name: r.ident.to_string(),
            methods: r.methods.iter().map(Method::from).collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Use {
    pub span: Span,
    pub name: String,
    pub methods: Vec<Method>,
}

impl<'a> From<&crate::Use<'a>> for Use {
    fn from(u: &crate::Use<'a>) -> Self {
        Self {
            span: u.statement.span().into(),
            name: u.ident.to_string(),
            methods: u.methods.iter().map(Method::from).collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Item {
    Definition(Definition),
    Redefinition(Redefinition),
    Use(Use),
}

impl<'a> From<&crate::Item<'a>> for Item {
    fn from(item: &crate::Item<'a>) -> Self {
        match item {
            crate::Item::Definition(item) => Self::Definition(item.into()),
            crate::Item::Redefinition(item) => Self::Redefinition(item.into()),
            crate::Item::Use(item) => Self::Use(item.into()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Function {
    pub name: String,
    pub span: Span,
    pub items: Vec<Item>,
}

impl<'a> From<&crate::Function<'a>> for Function {
    fn from(f: &crate::Function<'a>) -> Self {
        Self {
            name: f.ident.to_string(),
            span: Span {
                start: f.f.sig.span().start().into(),
                end: f.f.block.span().end().into(),
            },
            items: f
                .items
                .iter()
                .flat_map(|(_, i)| i)
                .map(Item::from)
                .collect(),
        }
    }
}
