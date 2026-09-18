mod derive_error;

use derive_error::{typescript_error_declaration, typescript_error_definition};
use proc_macro::TokenStream;
use proc_macro2::Span;
use proc_macro_error::abort_call_site;
use quote::quote;
use syn::{parse_macro_input, Field, Fields, ItemStruct, LitStr};

use crate::derive_error::TypescriptErrorField;

#[proc_macro_attribute]
pub fn typescript_error(_meta: TokenStream, input: TokenStream) -> TokenStream {
    let ItemStruct { attrs, vis, ident, generics: _, fields, .. } =
        parse_macro_input!(input as ItemStruct);

    let typescript_fields = match &fields {
        Fields::Named(fields) => fields
            .named
            .iter()
            .flat_map(TypescriptErrorField::new)
            .collect(),
        Fields::Unit => vec![],
        Fields::Unnamed { .. } => {
            abort_call_site!(
                "#[derive(TypescriptError)] can only be used for structs with named fields"
            )
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

    let output = quote! {
        #[wasm_bindgen::prelude::wasm_bindgen(typescript_custom_section)]
        const _: &'static str = #typescript_literal;

        #[wasm_bindgen::prelude::wasm_bindgen(inline_js = #javascript_literal)]
        extern "C" {
            #[wasm_bindgen(extends = js_sys::Error)]
            #(#attrs)* #vis type #ident;

            #[wasm_bindgen(constructor)]
            pub fn new(message: &str, #(#rust_constructor_params),*) -> #ident;
        }
    };
    output.into()
}
