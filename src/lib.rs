use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use quote::quote;

#[proc_macro_derive(Reactive)]
pub fn task(input: TokenStream) -> TokenStream {
    // Parse tokens directly to preserve spans in compiler diagnostics.
    let ast = syn::parse_macro_input!(input as syn::DeriveInput);

    // Resolve renamed dependencies in the consuming crate; the macro itself
    // does not need to link Flux or Bevy at build time.
    let found_crate = crate_name("flux").expect("flux is not found in Cargo.toml");

    let prelude = match found_crate {
        FoundCrate::Itself => quote!(crate::prelude),
        FoundCrate::Name(ref flux_name) => {
            let ident = syn::Ident::new(flux_name, proc_macro2::Span::call_site());
            quote!(::#ident::prelude)
        }
    };

    // Build the impl
    let name = &ast.ident;
    let quote = quote! {

        #prelude::enable_global_type_registration!(#name);

        impl #prelude::Reactive for #name {
        }
    };
    TokenStream::from(quote)
}
