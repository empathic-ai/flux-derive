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
mod query;
mod query_expr;

/// A checked reflected property path, excluding the root type name.
#[proc_macro]
pub fn property_path(input: TokenStream) -> TokenStream {
    let path = syn::parse_macro_input!(input as binding::PropertyPath);
    binding::flux()
        .map(|flux| path.expand(&flux, false))
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

/// A checked entity/component/property location returning BindingResult<TypedBindingPath<T>>.
#[proc_macro]
pub fn path(input: TokenStream) -> TokenStream {
    let path = syn::parse_macro_input!(input as binding::Location);
    binding::flux()
        .map(|flux| path.expand(&flux))
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

/// Compose sources, existing nodes, typed adapters, paths, and ECS systems.
#[proc_macro]
pub fn binding_node(input: TokenStream) -> TokenStream {
    let pipeline = syn::parse_macro_input!(input as binding::Pipeline);
    binding::flux()
        .map(|flux| pipeline.expand(&flux))
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

/// Build a typed query expression for zero or more ECS or database records.
///
/// The expression can be passed to `Commands::query` for ECS execution or to
/// `Commands::db_query` for SurrealDB execution:
///
/// ```ignore
/// let expression = query!(UserRecord WHERE code = user_code);
/// commands.query(expression, move |In(records): In<Vec<(Id, UserRecord)>>| {
///     // Other Bevy system parameters can follow the input.
/// });
/// ```
#[proc_macro]
pub fn query(input: TokenStream) -> TokenStream {
    let query = syn::parse_macro_input!(input as query_expr::QueryExprInput);
    query::flux()
        .map(|flux| query.expand(&flux, false))
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

/// Build a typed query expression for zero or one ECS or database records.
///
/// ```ignore
/// let expression = query_one!(UserRecord WHERE code = user_code LIMIT 1);
/// commands.db_query_one(
///     expression,
///     move |In(record): In<Option<(Id, UserRecord)>>| {
///         // Other Bevy system parameters can follow the input.
///     },
/// );
/// ```
#[proc_macro]
pub fn query_one(input: TokenStream) -> TokenStream {
    let query = syn::parse_macro_input!(input as query_expr::QueryExprInput);
    query::flux()
        .map(|flux| query.expand(&flux, true))
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

/// Build an ECS-only query expression from an arbitrary typed Rust predicate.
///
/// ```ignore
/// let expression = bevy_query!(UserRecord, |record: &UserRecord| {
///     record.code.starts_with("USR-")
/// });
/// commands.query(expression, handler);
/// ```
///
/// Structural Bevy filters can be supplied before the predicate:
///
/// ```ignore
/// let expression = bevy_query!(
///     UserRecord,
///     (With<Active>, Without<Archived>, Changed<UserRecord>),
///     |record: &UserRecord| record.code.starts_with("USR-")
/// );
/// ```
#[proc_macro]
pub fn bevy_query(input: TokenStream) -> TokenStream {
    let query = syn::parse_macro_input!(input as query_expr::BevyQueryExprInput);
    query::flux()
        .map(|flux| query.expand(&flux, false))
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

/// Build an ECS-only one-result query expression from an arbitrary typed Rust predicate.
#[proc_macro]
pub fn bevy_query_one(input: TokenStream) -> TokenStream {
    let query = syn::parse_macro_input!(input as query_expr::BevyQueryExprInput);
    query::flux()
        .map(|flux| query.expand(&flux, true))
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

/// Run a typed SurrealQL query and pass all returned records to a Bevy system.
///
/// ```ignore
/// surreal_query!(
///     commands,
///     UserRecord,
///     "SELECT * FROM user_record WHERE code = $code",
///     { code: user_code },
///     move |In(records): In<Vec<(Id, UserRecord)>>| { /* ... */ },
/// );
/// ```
#[proc_macro]
pub fn surreal_query(input: TokenStream) -> TokenStream {
    let query = syn::parse_macro_input!(input as query::Query);
    query::flux()
        .map(|flux| query.expand_many(&flux))
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

/// Run a typed SurrealQL query and pass its first optional record to a Bevy system.
///
/// ```ignore
/// surreal_query_one!(
///     commands,
///     UserRecord,
///     "SELECT * FROM user_record WHERE code = $code LIMIT 1",
///     { code: user_code },
///     move |In(record): In<Option<(Id, UserRecord)>>| { /* ... */ },
/// );
/// ```
#[proc_macro]
pub fn surreal_query_one(input: TokenStream) -> TokenStream {
    let query = syn::parse_macro_input!(input as query::Query);
    query::flux()
        .map(|flux| query.expand_one(&flux))
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

/// A checked component/property location without an entity, returning
/// BindingResult<TypedComponentBindingPath<T>>.
#[proc_macro]
pub fn component_path(input: TokenStream) -> TokenStream {
    let path = syn::parse_macro_input!(input as binding::PropertyPath);

    binding::flux()
        .map(|flux| {
            let root = path.root();
            let property = path.expand(&flux, false);
            let projection = path.projection();

            quote!({
                let __flux_property = #property;

                #flux::binding::ComponentBindingPath::new(
                    #flux::binding::__macro_support::component_name::<#root>(),
                    if __flux_property.is_empty() {
                        None
                    } else {
                        Some(__flux_property.as_str())
                    },
                )
                .map(|path| {
                    #flux::binding::TypedComponentBindingPath::from_projection(
                        path,
                        #projection,
                    )
                })
            })
        })
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
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
