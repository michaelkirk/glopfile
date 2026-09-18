mod derive_error;

use derive_error::{typescript_error_declaration, typescript_error_definition};
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{parse_macro_input, Error, Field, Fields, ItemStruct, LitStr, Result};

use crate::derive_error::TypescriptErrorField;

#[proc_macro_attribute]
pub fn typescript_error(_meta: TokenStream, input: TokenStream) -> TokenStream {
    let item = parse_macro_input!(input as ItemStruct);
    match expand_typescript_error(item) {
        Ok(output) => output.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_typescript_error(item: ItemStruct) -> Result<proc_macro2::TokenStream> {
    let ItemStruct { attrs, vis, ident, generics: _, fields, .. } = item;

    let typescript_fields = match &fields {
        Fields::Named(fields) => fields
            .named
            .iter()
            .filter_map(|field| TypescriptErrorField::new(field).transpose())
            .collect::<Result<Vec<_>>>()?,
        Fields::Unit => vec![],
        Fields::Unnamed { .. } => {
            return Err(Error::new_spanned(
                &fields,
                "#[typescript_error] can only be used for structs with named fields",
            ))
        }
    };

    let typescript = typescript_error_declaration(&ident, &typescript_fields);
    let typescript_literal = LitStr::new(&typescript, Span::call_site());

    let javascript = typescript_error_definition(&ident, &typescript_fields);
    let javascript_literal = LitStr::new(&javascript, Span::call_site());

    let rust_constructor_params = fields.iter().map(|Field { ident, ty, .. }| {
        quote! {
            #ident: #ty
        }
    });

    Ok(quote! {
        #[wasm_bindgen::prelude::wasm_bindgen(typescript_custom_section)]
        const _: &'static str = #typescript_literal;

        #[wasm_bindgen::prelude::wasm_bindgen(inline_js = #javascript_literal)]
        extern "C" {
            #[wasm_bindgen(extends = js_sys::Error)]
            #(#attrs)* #vis type #ident;

            #[wasm_bindgen(constructor)]
            pub fn new(message: &str, #(#rust_constructor_params),*) -> #ident;
        }
    })
}
