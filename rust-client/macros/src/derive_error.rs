use std::fmt::Write;

use syn::{Error, Expr, ExprLit, Field, Ident, Lit, Meta, MetaNameValue, Result};

pub struct TypescriptErrorField {
    pub name: String,
    pub ty: String,
}

impl TypescriptErrorField {
    pub fn new(field: &Field) -> Result<Option<Self>> {
        let Some(attr) = field
            .attrs
            .iter()
            .find(|attr| attr.path().is_ident("typescript_type"))
        else {
            return Ok(None);
        };
        let ty = match &attr.meta {
            Meta::NameValue(MetaNameValue {
                value: Expr::Lit(ExprLit { lit: Lit::Str(lit_str), .. }),
                ..
            }) => lit_str.value(),
            meta => {
                return Err(Error::new_spanned(
                    meta,
                    "usage: #[typescript_type = \"...\"]",
                ))
            }
        };
        let name = field.ident.as_ref().unwrap().to_string();
        Ok(Some(Self { name, ty }))
    }
}

pub fn typescript_error_declaration<'a>(
    class_name: &Ident,
    fields: impl IntoIterator<Item = &'a TypescriptErrorField> + Clone,
) -> String {
    let mut out = String::new();

    let field_specs = fields
        .into_iter()
        .map(|TypescriptErrorField { name, ty }| format!("{name}: {ty}"))
        .collect::<Vec<_>>();

    writeln!(out, "export interface {class_name} {{").unwrap();
    for field_spec in &field_specs {
        writeln!(out, "  {field_spec};").unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out, "export interface {class_name}Name {{ {class_name} }}").unwrap();

    out
}

pub fn typescript_error_definition<'a>(
    class_name: &Ident,
    fields: impl IntoIterator<Item = &'a TypescriptErrorField>,
) -> String {
    let mut out = String::new();

    let field_names = fields
        .into_iter()
        .map(|TypescriptErrorField { name, .. }| name.to_string())
        .collect::<Vec<_>>();

    writeln!(out, "export class {class_name} extends Error {{").unwrap();

    let constructor_params = field_names.join(", ");
    writeln!(out, "  constructor(message, {constructor_params}) {{").unwrap();
    writeln!(out, "    super(message);").unwrap();
    writeln!(out, "    this.name = \"{class_name}\";").unwrap();
    for name in field_names {
        writeln!(out, "    this.{name} = {name};").unwrap();
    }
    writeln!(out, "  }}").unwrap();
    writeln!(out, "}}").unwrap();

    out
}
