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

mod binding;

/// A checked reflected property path, excluding the root type name.
#[proc_macro]
pub fn property_path(input: TokenStream) -> TokenStream {
    let path = syn::parse_macro_input!(input as binding::PropertyPath);
    binding::flux().map(|flux| path.expand(&flux, false))
        .unwrap_or_else(|error| error.to_compile_error()).into()
}

/// A checked entity/component/property location returning BindingResult<BindingPath>.
#[proc_macro]
pub fn binding_path(input: TokenStream) -> TokenStream {
    let path = syn::parse_macro_input!(input as binding::Location);
    binding::flux().map(|flux| path.expand(&flux))
        .unwrap_or_else(|error| error.to_compile_error()).into()
}

/// Compose sources, existing nodes, typed adapters, paths, and ECS systems.
#[proc_macro]
pub fn binding_node(input: TokenStream) -> TokenStream {
    let pipeline = syn::parse_macro_input!(input as binding::Pipeline);
    binding::flux().map(|flux| pipeline.expand(&flux))
        .unwrap_or_else(|error| error.to_compile_error()).into()
}

/// A checked component/property location without an entity, for builder targets.
#[proc_macro]
pub fn component_path(input: TokenStream) -> TokenStream {
    let path = syn::parse_macro_input!(input as binding::PropertyPath);
    binding::flux().map(|flux| {
        let root = path.root();
        let path = path.expand(&flux, false);
        quote!({
            let __flux_property = #path;
            #flux::binding::ComponentBindingPath::new(
                #flux::binding::__macro_support::component_name::<#root>(),
                if __flux_property.is_empty() { None } else { Some(__flux_property.as_str()) },
            )
        })
    }).unwrap_or_else(|error| error.to_compile_error()).into()
}

/// Creates a Bevy `HashMap` from comma-separated key/value expressions.
///
/// Bevy's `HashMap` is a platform-selected type rather than
/// `std::collections::HashMap`, so this deliberately builds the collection
/// through `FromIterator` and lets the surrounding type annotation select the
/// concrete map type.
#[proc_macro]
pub fn bevy_hash_map(input: TokenStream) -> TokenStream {
    let entries = syn::parse_macro_input!(input with syn::punctuated::Punctuated::<MapEntry, syn::Token![,]>::parse_terminated);
    let entries = entries.iter().map(|entry| {
        let key = &entry.key;
        let value = &entry.value;
        quote!((#key, #value))
    });

    quote! {
        ::core::iter::IntoIterator::into_iter([#(#entries),*]).collect()
    }
    .into()
}

struct MapEntry {
    key: syn::Expr,
    value: syn::Expr,
}

impl syn::parse::Parse for MapEntry {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let key = input.parse()?;
        input.parse::<syn::Token![=>]>()?;
        let value = input.parse()?;
        Ok(Self { key, value })
    }
}
