//! Checked Rust field paths and graph composition; no runtime dependencies here.
use proc_macro2::TokenStream;
use quote::{quote, quote_spanned};
use syn::{
    Expr, Ident, LitInt, Member, Path, Token, TypePath, bracketed,
    ext::IdentExt,
    parenthesized,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    spanned::Spanned,
};

pub fn flux() -> syn::Result<TokenStream> {
    match proc_macro_crate::crate_name("flux") {
        Ok(proc_macro_crate::FoundCrate::Itself) => Ok(quote!(::flux)),
        Ok(proc_macro_crate::FoundCrate::Name(name)) => {
            let name = Ident::new(&name, proc_macro2::Span::call_site());
            Ok(quote!(::#name))
        }
        Err(error) => Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            error.to_string(),
        )),
    }
}

enum Access {
    Field(Member),
    Index(LitInt),
    Unwrap(Token![?]),
}
struct Segment {
    ty: TypePath,
    accesses: Vec<Access>,
    jump: bool,
}
pub struct PropertyPath {
    segments: Vec<Segment>,
}

impl Parse for PropertyPath {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut segments = Vec::new();
        let mut jump = false;
        loop {
            let ty = input.parse()?;
            let mut accesses = Vec::new();
            loop {
                if input.peek(Token![.]) {
                    input.parse::<Token![.]>()?;
                    accesses.push(Access::Field(input.parse()?));
                } else if input.peek(syn::token::Bracket) {
                    let content;
                    bracketed!(content in input);
                    let index = content.parse::<LitInt>()?;
                    if !index.suffix().is_empty() {
                        return Err(syn::Error::new(
                            index.span(),
                            "Use an unsuffixed literal index",
                        ));
                    }
                    if !content.is_empty() {
                        return Err(content.error("Expected one literal index"));
                    }
                    accesses.push(Access::Index(index));
                } else if input.peek(Token![?]) {
                    accesses.push(Access::Unwrap(input.parse()?));
                } else {
                    break;
                }
            }
            if matches!(accesses.last(), Some(Access::Unwrap(_))) {
                return Err(input.error("Use ? before another field/index access; a final Option is already a bindable value"));
            }
            segments.push(Segment { ty, accesses, jump });
            if input.peek(Token![->]) {
                input.parse::<Token![->]>()?;
                jump = true;
            } else if input.peek(Token![as]) {
                input.parse::<Token![as]>()?;
                jump = false;
            } else {
                break;
            }
        }
        Ok(Self { segments })
    }
}

impl PropertyPath {
    pub fn root(&self) -> &TypePath {
        &self.segments[0].ty
    }
    /// A never-executed projection lets Rust infer the leaf type, including
    /// indices, Option traversal, explicit dynamic shapes, and entity jumps.
    pub(crate) fn projection(&self) -> TokenStream {
        let segment = self.segments.last().unwrap();
        let ty = &segment.ty;
        let mut access = quote!((*__flux_value));
        for step in &segment.accesses {
            access = match step {
                Access::Field(member) => quote_spanned!(member.span()=> #access.#member),
                Access::Index(index) => quote_spanned!(index.span()=> #access[#index]),
                Access::Unwrap(token) => quote_spanned!(token.span()=> #access.as_ref().unwrap()),
            };
        }
        quote!(|__flux_value: &#ty| { &#access })
    }

    pub fn expand(&self, flux: &TokenStream, include_root: bool) -> TokenStream {
        let mut statements = Vec::new();
        for (index, segment) in self.segments.iter().enumerate() {
            let ty = &segment.ty;
            let mut access = TokenStream::new();
            let mut spelling = String::new();
            for step in &segment.accesses {
                match step {
                    Access::Field(member) => {
                        access.extend(quote_spanned!(member.span()=> .#member));
                        if !spelling.is_empty() {
                            spelling.push('.');
                        }
                        spelling.push_str(&match member {
                            Member::Named(name) => name.unraw().to_string(),
                            Member::Unnamed(index) => index.index.to_string(),
                        });
                    }
                    Access::Index(index) => {
                        access.extend(quote_spanned!(index.span()=> [#index]));
                        spelling.push_str(&format!("[{}]", index.base10_digits()));
                    }
                    Access::Unwrap(token) => {
                        access.extend(quote_spanned!(token.span()=> .as_ref().unwrap()));
                    }
                }
            }
            // Never called: normal Rust field access performs the type/privacy check.
            // At a -> boundary also require the preceding value to be a Flux Id.
            let check = if self.segments.get(index + 1).is_some_and(|s| s.jump) {
                quote!(#flux::binding::__macro_support::assert_id(&(*__flux_value) #access);)
            } else {
                quote!(let _ = &__flux_value #access;)
            };
            statements.push(quote! { let _ = |__flux_value: &#ty| { #check }; });
            if segment.jump || (index == 0 && include_root) {
                statements.push(quote! {
                    if !__flux_path.is_empty() { __flux_path.push('.'); }
                    __flux_path.push_str(#flux::binding::__macro_support::component_name::<#ty>());
                });
            }
            if !spelling.is_empty() {
                let starts_index = spelling.starts_with('[');
                statements.push(quote! {
                    if !__flux_path.is_empty() && !#starts_index { __flux_path.push('.'); }
                    __flux_path.push_str(#spelling);
                });
            }
        }
        quote!({
            let mut __flux_path = ::std::string::String::new();
            #(#statements)*
            __flux_path
        })
    }
}

pub struct Location {
    entity: Expr,
    path: PropertyPath,
}
impl Parse for Location {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let entity = input.parse()?;
        input.parse::<Token![,]>()?;
        let path = input.parse()?;
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }
        Ok(Self { entity, path })
    }
}
impl Location {
    pub fn expand(&self, flux: &TokenStream) -> TokenStream {
        let entity = &self.entity;
        let root = self.path.root();
        let path = self.path.expand(flux, false);
        let projection = self.path.projection();
        quote!({
            let __flux_entity = #entity;
            let __flux_property = #path;
            #flux::binding::BindingPath::new(
                __flux_entity,
                #flux::binding::__macro_support::component_name::<#root>(),
                if __flux_property.is_empty() { None } else { Some(__flux_property.as_str()) },
            ).map(|path| #flux::binding::TypedBindingPath::from_projection(path, #projection))
        })
    }
}

enum Start {
    Source(Location),
    Node(Expr),
    System(Expr, Expr),
}
enum Stage {
    Path(PropertyPath, bool),
    Method(Path, Expr),
    System(Expr, Expr),
    Then(Expr),
}
pub struct Pipeline {
    graph: Expr,
    start: Start,
    stages: Vec<Stage>,
}
fn pair(input: ParseStream) -> syn::Result<(Expr, Expr)> {
    let args = Punctuated::<Expr, Token![,]>::parse_terminated(input)?;
    if args.len() != 2 {
        return Err(input.error("Expected name and system"));
    }
    let mut args = args.into_iter();
    Ok((args.next().unwrap(), args.next().unwrap()))
}
impl Parse for Pipeline {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let graph = input.parse()?;
        input.parse::<Token![;]>()?;
        let name: Ident = input.parse()?;
        let content;
        parenthesized!(content in input);
        let start = match name.to_string().as_str() {
            "source" => Start::Source(content.parse()?),
            "node" => Start::Node(content.parse()?),
            "system" => {
                let (name, system) = pair(&content)?;
                Start::System(name, system)
            }
            _ => {
                return Err(syn::Error::new(
                    name.span(),
                    "Start with source(entity, Type.field), node(existing), or system(name, function)",
                ));
            }
        };
        if !content.is_empty() {
            return Err(content.error("Unexpected tokens in source"));
        }
        let mut stages = Vec::new();
        while !input.is_empty() {
            input.parse::<Token![=>]>()?;
            let ident: Ident = input.parse()?;
            let mut method = Path::from(ident);
            if input.peek(Token![::]) {
                let colon = input.parse::<Token![::]>()?;
                let mut arguments: syn::AngleBracketedGenericArguments = input.parse()?;
                arguments.colon2_token = Some(colon);
                method.segments.last_mut().unwrap().arguments =
                    syn::PathArguments::AngleBracketed(arguments);
            }
            if method.segments.len() != 1 {
                return Err(syn::Error::new(
                    method.span(),
                    "Expected a pipeline stage name",
                ));
            }
            let name = method.segments[0].ident.to_string();
            let content;
            parenthesized!(content in input);
            let stage = match name.as_str() {
                "path" => Stage::Path(content.parse()?, false),
                "jump" => Stage::Path(content.parse()?, true),
                "filter" | "map_value" => Stage::Method(method, content.parse()?),
                "system" => {
                    let (name, system) = pair(&content)?;
                    Stage::System(name, system)
                }
                "then" => Stage::Then(content.parse()?),
                _ => {
                    return Err(syn::Error::new(
                        method.span(),
                        "Expected filter, map_value, path, jump, system, or then",
                    ));
                }
            };
            if !content.is_empty() {
                return Err(content.error("Unexpected stage arguments"));
            }
            stages.push(stage);
        }
        Ok(Self {
            graph,
            start,
            stages,
        })
    }
}
impl Pipeline {
    pub fn expand(&self, flux: &TokenStream) -> TokenStream {
        let graph = &self.graph;
        let start = match &self.start {
            Start::Source(location) => {
                let location = location.expand(flux);
                quote!(__flux_graph.source(#location?)?)
            }
            Start::Node(node) => quote!(#node),
            Start::System(name, system) => quote!(__flux_graph.system(#name, &[], #system)?),
        };
        let stages = self.stages.iter().map(|stage| match stage {
            Stage::Path(path, include_root) => {
                let path = path.expand(flux, *include_root);
                quote!(__flux_graph.path(__flux_node, &#path)?)
            }
            Stage::Method(method, arg) => quote!(__flux_graph.#method(__flux_node, #arg)?),
            Stage::System(name, system) => {
                quote!(__flux_graph.system(#name, &[__flux_node], #system)?)
            }
            Stage::Then(func) => quote!((#func)(__flux_graph, __flux_node)?),
        });
        quote!((|| -> #flux::binding::BindingResult<#flux::binding::BindingNode> {
            let __flux_graph = &mut (#graph);
            let __flux_node = #start;
            #(let __flux_node = #stages;)*
            Ok(__flux_node)
        })())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_paths_and_distinguishes_dynamic_shapes_from_jumps() {
        let path: PropertyPath =
            syn::parse_str("View.value -> Device.configs[0] as Config.ssid").unwrap();
        assert_eq!(path.segments.len(), 3);
        assert!(path.segments[1].jump);
        assert!(!path.segments[2].jump);
        assert!(syn::parse_str::<PropertyPath>("Model.items[index]").is_err());
        assert!(syn::parse_str::<PropertyPath>("Model.value?").is_err());
    }
    #[test]
    fn parses_pipeline_and_rejects_unknown_operations() {
        assert!(syn::parse_str::<Pipeline>("graph; source(entity, Model.items) => filter::<i32>(|v| *v > 0) => path(Item.value)").is_ok());
        assert!(syn::parse_str::<Pipeline>("graph; node(a) => unknown(foo)").is_err());
    }
}
