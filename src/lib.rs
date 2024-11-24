extern crate proc_macro;
extern crate syn;
#[macro_use]
extern crate quote;

use ragent_core::prelude::*;
use proc_macro::TokenStream;

#[proc_macro_derive(Task)]
pub fn task(input: TokenStream) -> TokenStream {
    // Construct a string representation of the type definition
    let s = input.to_string();
    
    // Parse the string representation
    let ast: syn::DeriveInput = syn::parse_str(&s).unwrap();

    // Build the impl
    let name = &ast.ident;
    let gen = quote! {
        impl ragent_core::prelude::Task for #name {
        }
    };
    TokenStream::from(gen)
}