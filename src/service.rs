use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{spanned::Spanned, FnArg, GenericArgument, ItemTrait, PathArguments, ReturnType, TraitItem, Type};

pub fn expand(service: ItemTrait) -> syn::Result<TokenStream> {
    let reject = |span, message| syn::Error::new(span, message);
    if !service.generics.params.is_empty() || !service.supertraits.is_empty()
        || service.generics.where_clause.is_some() || service.unsafety.is_some() || service.auto_token.is_some() {
        return Err(reject(service.span(), "Service declarations cannot have generics or supertraits"));
    }
    let path = match proc_macro_crate::crate_name("flux")
        .map_err(|error| syn::Error::new(service.span(), error.to_string()))? {
        proc_macro_crate::FoundCrate::Itself => quote!(::flux::prelude),
        proc_macro_crate::FoundCrate::Name(name) => {
            let name = format_ident!("{}", name);
            quote!(::#name::prelude)
        }
    };
    let name = &service.ident;
    let visibility = &service.vis;
    let client = format_ident!("{}Client", name);
    let remote_client = format_ident!("{}RemoteClient", name);
    let handlers = format_ident!("{}Handlers", name);
    let mut associations = Vec::new();
    let mut methods = Vec::new();
    let mut fields = Vec::new();
    let mut parameters = Vec::new();
    let mut names = Vec::new();
    let mut generics = Vec::new();
    let mut markers = Vec::new();
    let mut bounds = Vec::new();
    let mut registrations = Vec::new();
    let mut remote_methods = Vec::new();
    let mut wire_registrations = Vec::new();
    for (index, item) in service.items.iter().enumerate() {
        let TraitItem::Fn(method) = item else {
            return Err(reject(item.span(), "Only service methods are supported"));
        };
        let signature = &method.sig;
        if signature.asyncness.is_none() || signature.inputs.len() != 1
            || !signature.generics.params.is_empty() || method.default.is_some()
            || signature.unsafety.is_some() || signature.abi.is_some()
            || signature.generics.where_clause.is_some() || signature.constness.is_some()
            || signature.variadic.is_some() {
            return Err(reject(signature.span(), "Expected async fn method(request: Request) -> Result<Response, Error>"));
        }
        let FnArg::Typed(input) = &signature.inputs[0] else {
            return Err(reject(signature.span(), "Service declarations do not take self"));
        };
        let request = &input.ty;
        let ReturnType::Type(_, output) = &signature.output else {
            return Err(reject(signature.span(), "Service methods must return Result<Response, Error>"));
        };
        let Type::Path(output) = output.as_ref() else {
            return Err(reject(output.span(), "Expected Result<Response, Error>"));
        };
        let segment = output.path.segments.last().unwrap();
        let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
            return Err(reject(output.span(), "Expected Result<Response, Error>"));
        };
        if segment.ident != "Result" || arguments.args.len() != 2 {
            return Err(reject(output.span(), "Expected Result<Response, Error>"));
        }
        let (GenericArgument::Type(response), GenericArgument::Type(error)) = (&arguments.args[0], &arguments.args[1]) else {
            return Err(reject(output.span(), "Expected response and error types"));
        };
        let system = method.attrs.iter().any(|attr| attr.path().is_ident("system"));
        let mut wire = None;
        for attr in &method.attrs {
            if attr.path().is_ident("wire") {
                if wire.is_some() { return Err(reject(attr.span(), "Duplicate wire attribute")); }
                let operation: syn::LitStr = attr.parse_args()?;
                if operation.value().is_empty() { return Err(reject(attr.span(), "Wire operation ID cannot be empty")); }
                wire = Some(operation);
            } else if !attr.path().is_ident("system") && !attr.path().is_ident("doc") {
                return Err(reject(attr.span(), "Unsupported service method attribute"));
            }
        }
        let method_name = &signature.ident;
        if let Some(operation) = wire {
            associations.push(quote! {
                impl #path::WireRequest for #request { const OPERATION: &'static str = #operation; }
            });
            wire_registrations.push(quote! {
                app.init_resource::<#path::WireServiceRegistry>();
                app.world_mut().resource_mut::<#path::WireServiceRegistry>().expose::<#request>();
            });
            remote_methods.push(quote! {
                #visibility fn #method_name(&self, request: #request) -> #path::TaskResult<#response, #path::CallError<#error>> {
                    self.endpoint.call(request)
                }
            });
        }
        let handler_type = format_ident!("Handler{}", index);
        let marker = format_ident!("Marker{}", index);
        generics.push(handler_type.clone());
        markers.push(marker.clone());
        names.push(method_name.clone());
        fields.push(quote!(#method_name: #handler_type));
        parameters.push(quote!(#method_name: #handler_type));
        associations.push(quote! {
            impl #path::ServiceRequest for #request {
                type Response = #response;
                type Error = #error;
            }
        });
        methods.push(quote! {
            #visibility fn #method_name(&self, request: #request) -> #path::TaskResult<#response, #path::CallError<#error>> {
                self.registry.call(self.executor.clone(), #path::RequestContext {
                    peer_id: self.peer_id, request_id: #path::Id::new(),
                }, request)
            }
        });
        if system {
            bounds.push(quote! {
                #error: From<#path::ExecuteError>,
                #handler_type: #path::IntoSystem<#path::In<(#path::RequestContext, #request)>, #path::TaskResult<#response, #error>, #marker>
                    + Clone + Send + Sync + 'static
            });
            registrations.push(quote!(#path::ServiceAppExt::register_system_handler::<#request, _, #marker>(app, self.#method_name);));
        } else {
            bounds.push(quote! {
                #handler_type: Fn(#path::Executor, #path::RequestContext, #request) -> #marker + Send + Sync + 'static,
                #marker: ::std::future::Future<Output = Result<#response, #error>> + Send + 'static
            });
            registrations.push(quote!(#path::ServiceAppExt::register_handler::<#request, _, _>(app, self.#method_name);));
        }
    }
    if generics.is_empty() {
        return Err(reject(service.span(), "A service must declare at least one method"));
    }
    Ok(quote! {
        #visibility struct #name;
        #(#associations)*
        #[derive(Clone)]
        #visibility struct #client {
            registry: #path::ServiceRegistry,
            executor: #path::Executor,
            peer_id: #path::Id,
        }
        impl #client {
            #visibility fn new(registry: #path::ServiceRegistry, executor: #path::Executor, peer_id: #path::Id) -> Self {
                Self { registry, executor, peer_id }
            }
            #(#methods)*
        }
        #[derive(Clone)]
        #visibility struct #remote_client { endpoint: #path::RemoteServiceClient }
        impl #remote_client {
            #visibility fn new(endpoint: #path::RemoteServiceClient) -> Self { Self { endpoint } }
            #(#remote_methods)*
        }
        #visibility struct #handlers<#(#generics,)* #(#markers,)*> {
            #(#fields,)*
            marker: ::std::marker::PhantomData<fn() -> (#(#markers,)*)>,
        }
        impl #name {
            #visibility fn handlers<#(#generics,)* #(#markers,)*>(#(#parameters),*) -> #handlers<#(#generics,)* #(#markers,)*>
            where #(#bounds,)* {
                #handlers { #(#names,)* marker: ::std::marker::PhantomData }
            }
        }
        impl<#(#generics,)* #(#markers,)*> #path::Service for #handlers<#(#generics,)* #(#markers,)*>
        where #(#bounds,)* {
            fn register(self, app: &mut #path::App) { #(#registrations)* #(#wire_registrations)* }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsupported_service_declarations() {
        for source in [
            "trait Empty {}",
            "trait Generic<T> { async fn call(request: T) -> Result<(), ()>; }",
            "trait Receiver { async fn call(&self) -> Result<(), ()>; }",
            "trait Sync { fn call(request: Request) -> Result<(), ()>; }",
            "trait Output { async fn call(request: Request) -> (); }",
            "trait Attribute { #[unknown] async fn call(request: Request) -> Result<(), ()>; }",
        ] {
            assert!(expand(syn::parse_str(source).unwrap()).is_err(), "{source}");
        }
    }
}