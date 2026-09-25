use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Expr, Ident, LitStr, Token, Type, braced,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

use crate::binding;

struct Variable {
    name: Ident,
    value: Expr,
}

impl Parse for Variable {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        Ok(Self {
            name: input.parse()?,
            value: {
                input.parse::<Token![:]>()?;
                input.parse()?
            },
        })
    }
}

pub struct Query {
    commands: Expr,
    record: Type,
    statement: LitStr,
    variables: Vec<Variable>,
    callback: Expr,
}

impl Parse for Query {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let commands = input.parse()?;
        input.parse::<Token![,]>()?;
        let record = input.parse()?;
        input.parse::<Token![,]>()?;
        let statement = input.parse()?;
        input.parse::<Token![,]>()?;

        let content;
        braced!(content in input);
        let variables = Punctuated::<Variable, Token![,]>::parse_terminated(&content)?
            .into_iter()
            .collect();

        input.parse::<Token![,]>()?;
        let callback = input.parse()?;

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }
        if !input.is_empty() {
            return Err(input.error("Unexpected tokens after query callback"));
        }

        Ok(Self {
            commands,
            record,
            statement,
            variables,
            callback,
        })
    }
}

impl Query {
    fn expand(&self, flux: &TokenStream, one: bool) -> TokenStream {
        let commands = &self.commands;
        let record = &self.record;
        let statement = &self.statement;
        let callback = &self.callback;
        let variables = self.variables.iter().map(|variable| {
            let name = &variable.name;
            let value = &variable.value;
            quote!(#name: #value)
        });

        let method = if one {
            quote!(db_query_one_raw)
        } else {
            quote!(db_query_raw)
        };

        quote!({
            #flux::prelude::DbCommandsExt::#method::<#record, _, _, _>(
                &mut #commands,
                #statement,
                #flux::surrealdb_client::types::vars! { #(#variables),* },
                #callback,
            );
        })
    }

    pub fn expand_many(&self, flux: &TokenStream) -> TokenStream {
        self.expand(flux, false)
    }

    pub fn expand_one(&self, flux: &TokenStream) -> TokenStream {
        self.expand(flux, true)
    }
}

pub fn flux() -> syn::Result<TokenStream> {
    binding::flux()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn parses_query_with_multiple_bindings_and_trailing_comma() {
        let query: Query = syn::parse_str(
            r#"commands, PrivateDevice, "SELECT * FROM private_device WHERE code = $code AND owner = $owner", { code: device_code, owner: owner_id, }, move |result| { let _ = result; },"#,
        )
        .unwrap();

        assert_eq!(query.variables.len(), 2);
        assert_eq!(query.variables[0].name, "code");
        assert_eq!(query.variables[1].name, "owner");
    }

    #[test]
    fn parses_query_without_bindings() {
        let query: Query = syn::parse_str(
            r#"commands, User, "SELECT * FROM user", {}, |result| { let _ = result; }"#,
        )
        .unwrap();

        assert!(query.variables.is_empty());
    }

    #[test]
    fn rejects_missing_binding_value() {
        assert!(syn::parse_str::<Query>(
            r#"commands, User, "SELECT * FROM user WHERE id = $id", { id: }, |result| { let _ = result; }"#,
        )
        .is_err());
    }

    #[test]
    fn expands_through_the_fully_qualified_commands_trait() {
        let query: Query = syn::parse_str(
            r#"commands, User, "SELECT * FROM user", {}, |result| { let _ = result; }"#,
        )
        .unwrap();
        let expanded = query.expand_many(&quote!(::flux));
        let expanded = expanded.to_string();

        assert!(expanded.contains("DbCommandsExt :: db_query_raw"));
        assert!(expanded.contains("& mut commands"));
    }
}
