use std::fmt::Write;

use proc_macro_error::abort_call_site;
use syn::{Field, Ident, Lit, Meta, MetaNameValue};

pub struct TypescriptErrorField {
    pub name: String,
    pub ty: String,
}

impl TypescriptErrorField {
    pub fn new(field: &Field) -> Option<Self> {
        let typescript_type_attr = field
            .attrs
            .iter()
            .filter(|attr| attr.path.is_ident("typescript_type"))
            .next()?;
        let ty = match typescript_type_attr.parse_meta().unwrap() {
            Meta::NameValue(MetaNameValue { lit: Lit::Str(lit_str), .. }) => lit_str.value(),
            _ => abort_call_site!("Usage: #[typescript_type = \"...\"]"),
        };
        let name = field.ident.as_ref().unwrap().to_string();
        Some(Self { name, ty })
    }
}

pub fn typescript_error_declaration<'a>(
    class_name: &Ident,
    fields: impl IntoIterator<Item = &'a TypescriptErrorField> + Clone,
) -> String {
    let mut out = String::new();

    let field_specs = fields
        .clone()
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
