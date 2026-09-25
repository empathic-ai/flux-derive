use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Expr, Ident, LitInt, Token, Type,
    ext::IdentExt,
    parse::{Parse, ParseStream},
};

#[derive(Clone, Copy)]
enum Comparison {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Comparison {
    fn sql(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
        }
    }

    fn rust_tokens(self, field: &Ident, value: &Ident) -> TokenStream {
        match self {
            Self::Eq => quote!(record.#field == #value),
            Self::Ne => quote!(record.#field != #value),
            Self::Lt => quote!(record.#field < #value),
            Self::Le => quote!(record.#field <= #value),
            Self::Gt => quote!(record.#field > #value),
            Self::Ge => quote!(record.#field >= #value),
        }
    }
}

struct Condition {
    field: Ident,
    comparison: Comparison,
    value: Expr,
}

impl Parse for Condition {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let field = input.call(Ident::parse_any)?;
        let comparison = if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            Comparison::Eq
        } else if input.peek(Token![!=]) {
            input.parse::<Token![!=]>()?;
            Comparison::Ne
        } else if input.peek(Token![<=]) {
            input.parse::<Token![<=]>()?;
            Comparison::Le
        } else if input.peek(Token![<]) {
            input.parse::<Token![<]>()?;
            Comparison::Lt
        } else if input.peek(Token![>=]) {
            input.parse::<Token![>=]>()?;
            Comparison::Ge
        } else if input.peek(Token![>]) {
            input.parse::<Token![>]>()?;
            Comparison::Gt
        } else {
            return Err(input.error("Expected a query comparison operator"));
        };

        Ok(Self {
            field,
            comparison,
            value: input.parse()?,
        })
    }
}

fn peek_word(input: ParseStream<'_>, word: &str) -> bool {
    let fork = input.fork();
    fork.call(Ident::parse_any)
        .map(|ident| ident.to_string().eq_ignore_ascii_case(word))
        .unwrap_or(false)
}

fn parse_word(input: ParseStream<'_>, word: &str) -> syn::Result<()> {
    let ident = input.call(Ident::parse_any)?;
    if ident.to_string().eq_ignore_ascii_case(word) {
        Ok(())
    } else {
        Err(syn::Error::new(ident.span(), format!("Expected `{word}`")))
    }
}

pub struct QueryExprInput {
    record: Type,
    conditions: Vec<Condition>,
    limit: Option<LitInt>,
}

pub struct BevyQueryExprInput {
    record: Type,
    filter: Option<Type>,
    predicate: Expr,
}

impl Parse for BevyQueryExprInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let record = input.parse()?;
        input.parse::<Token![,]>()?;

        let filter = {
            let fork = input.fork();
            match fork.parse::<Type>() {
                Ok(filter) if fork.peek(Token![,]) => {
                    input.parse::<Type>()?;
                    input.parse::<Token![,]>()?;
                    Some(filter)
                }
                _ => None,
            }
        };

        let predicate = input.parse()?;

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }
        if !input.is_empty() {
            return Err(input.error("Unexpected tokens after Bevy query predicate"));
        }

        Ok(Self {
            record,
            filter,
            predicate,
        })
    }
}

impl BevyQueryExprInput {
    pub fn expand(&self, flux: &TokenStream, one: bool) -> TokenStream {
        let record = &self.record;
        let predicate = &self.predicate;
        let filter = self
            .filter
            .as_ref()
            .map(|filter| quote!(#filter))
            .unwrap_or_else(|| quote!(()));
        let cardinality = if one {
            quote!(#flux::prelude::QueryOne)
        } else {
            quote!(#flux::prelude::QueryMany)
        };

        quote!({
            #flux::prelude::QueryExpr::<#record, #cardinality, (), _, #filter>::new(
                ::std::string::String::new(),
                (),
                #predicate,
                None,
            )
        })
    }
}

impl Parse for QueryExprInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let record = input.parse()?;
        let mut conditions = Vec::new();

        if peek_word(input, "where") {
            parse_word(input, "where")?;
            conditions.push(input.parse()?);

            while peek_word(input, "and") {
                parse_word(input, "and")?;
                conditions.push(input.parse()?);
            }
        }

        let limit = if peek_word(input, "limit") {
            parse_word(input, "limit")?;
            Some(input.parse()?)
        } else {
            None
        };

        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }
        if !input.is_empty() {
            return Err(input.error("Expected `WHERE`, `AND`, `LIMIT`, or the end of the query"));
        }

        Ok(Self {
            record,
            conditions,
            limit,
        })
    }
}

impl QueryExprInput {
    pub fn expand(&self, flux: &TokenStream, one: bool) -> TokenStream {
        let record = &self.record;
        let cardinality = if one {
            quote!(#flux::prelude::QueryOne)
        } else {
            quote!(#flux::prelude::QueryMany)
        };

        let declarations = self
            .conditions
            .iter()
            .enumerate()
            .map(|(index, condition)| {
                let binding = format_ident!("__flux_query_value_{index}");
                let value = &condition.value;
                quote!(let #binding = (#value);)
            });

        let variable_entries = self
            .conditions
            .iter()
            .enumerate()
            .map(|(index, condition)| {
                let binding = format_ident!("__flux_query_value_{index}");
                let field = &condition.field;
                quote!(#field: #binding.clone())
            });

        let predicates = self
            .conditions
            .iter()
            .enumerate()
            .map(|(index, condition)| {
                let binding = format_ident!("__flux_query_value_{index}");
                condition.comparison.rust_tokens(&condition.field, &binding)
            });

        let predicate = if predicates.len() == 0 {
            quote!(move |_record: &#record| true)
        } else {
            quote!(move |record: &#record| #(#predicates)&&*)
        };

        let mut suffix = String::new();
        if !self.conditions.is_empty() {
            suffix.push_str(" WHERE ");
            for (index, condition) in self.conditions.iter().enumerate() {
                if index > 0 {
                    suffix.push_str(" AND ");
                }
                suffix.push_str(&condition.field.to_string());
                suffix.push(' ');
                suffix.push_str(condition.comparison.sql());
                suffix.push_str(" $");
                suffix.push_str(&condition.field.to_string());
            }
        }
        if let Some(limit) = &self.limit {
            suffix.push_str(" LIMIT ");
            suffix.push_str(&limit.to_string());
        }

        let limit = self
            .limit
            .as_ref()
            .map(|limit| quote!(Some(#limit as usize)))
            .unwrap_or_else(|| quote!(None));

        quote!({
            #(#declarations)*
            let __flux_query_statement = ::std::format!(
                "SELECT * FROM {}{}",
                #flux::prelude::record_table::<#record>(),
                #suffix,
            );
            let __flux_query_variables =
                #flux::surrealdb_client::types::vars! { #(#variable_entries),* };
            #flux::prelude::QueryExpr::<#record, #cardinality, _, _>::new(
                __flux_query_statement,
                __flux_query_variables,
                #predicate,
                #limit,
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn parses_query_expression_with_case_insensitive_keywords() {
        let query: QueryExprInput = syn::parse_str(
            "PrivateDevice WHERE code = device_code AND device_id != ignored LIMIT 1",
        )
        .unwrap();

        assert_eq!(query.conditions.len(), 2);
        assert_eq!(query.limit.unwrap().base10_digits(), "1");
    }

    #[test]
    fn parses_query_expression_without_predicates() {
        let query: QueryExprInput = syn::parse_str("User").unwrap();
        assert!(query.conditions.is_empty());
        assert!(query.limit.is_none());
    }

    #[test]
    fn rejects_unknown_query_clause() {
        assert!(syn::parse_str::<QueryExprInput>("User ORDER BY name").is_err());
    }

    #[test]
    fn expands_a_typed_query_expression_and_keeps_bindings_parameterized() {
        let query: QueryExprInput =
            syn::parse_str("PrivateDevice WHERE code = device_code LIMIT 1").unwrap();
        let expanded = query.expand(&quote!(::flux), true).to_string();

        assert!(expanded.contains("QueryExpr"));
        assert!(expanded.contains("record_table"));
        assert!(expanded.contains("code : __flux_query_value_0 . clone"));
        assert!(expanded.contains("LIMIT 1"));
    }

    #[test]
    fn expands_a_bevy_only_query_expression() {
        let query: BevyQueryExprInput =
            syn::parse_str("PrivateDevice, |record: &PrivateDevice| record.code == code").unwrap();
        let expanded = query.expand(&quote!(::flux), false).to_string();

        assert!(expanded.contains("QueryExpr"));
        assert!(expanded.contains("String :: new"));
        assert!(expanded.contains("record . code == code"));
    }

    #[test]
    fn expands_a_bevy_query_with_structural_filters() {
        let query: BevyQueryExprInput = syn::parse_str(
            "PrivateDevice, (With<Active>, Without<Archived>, Changed<PrivateDevice>), |record: &PrivateDevice| record.code == code",
        )
        .unwrap();
        let expanded = query.expand(&quote!(::flux), false).to_string();

        assert!(expanded.contains("QueryExpr"));
        assert!(expanded.contains("With < Active >"));
        assert!(expanded.contains("Without < Archived >"));
        assert!(expanded.contains("Changed < PrivateDevice >"));
    }
}
