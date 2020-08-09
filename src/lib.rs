extern crate proc_macro;
use inflector::Inflector;
use proc_macro::TokenStream as RawTokenStream;
use proc_macro2::Span;
use quote::{quote, ToTokens};
use syn::{parse_macro_input, spanned::Spanned, TraitItem, TraitItemMethod};

#[proc_macro_attribute]
pub fn async_poll_trait(_attr: RawTokenStream, item: RawTokenStream) -> RawTokenStream {
    let mut parsed = parse_macro_input!(item as syn::ItemTrait);

    let mut poll_refs = Vec::new();
    let mut poll_owneds = Vec::new();

    let trait_name = &parsed.ident;

    for item in &mut parsed.items {
        if let TraitItem::Method(method) = item {
            for (attr_index, attr) in method.attrs.iter().enumerate() {
                if let Some(attr) = attr.path.get_ident() {
                    if attr == "async_ref" {
                        method.attrs.remove(attr_index);
                        poll_refs.push(&*method);
                        break;
                    } else if attr == "async_owned" {
                        method.attrs.remove(attr_index);
                        poll_owneds.push(&*method);
                        break;
                    }
                }
            }
        }
    }

    let mut future_defs = Vec::new();
    let mut method_defs = Vec::new();

    for method in poll_owneds {
        let poll_method_ident = &method.sig.ident;
        let poll_method_name = poll_method_ident.to_string();
        if !poll_method_name.starts_with("poll_") {
            return syn::Error::new(
                poll_method_ident.span(),
                "polling method must start with poll_",
            )
            .to_compile_error()
            .into();
        }
        let async_method_name = &poll_method_name[5..];
        let future_name = format!("{}{}", trait_name, async_method_name.to_class_case());

        let async_method_ident = syn::Ident::new(
            async_method_name,
            Span::call_site().resolved_at(poll_method_ident.span()),
        );

        let output = &method.sig.output;

        let poll_ready_type = match *output {
            syn::ReturnType::Default => {
                return syn::Error::new(output.span(), "polling method must return a Poll value")
                    .to_compile_error()
                    .into()
            }
            syn::ReturnType::Type(_, ref ty) => match **ty {
                syn::Type::Path(ref path) => {
                    let tail_segment = path.path.segments.last().unwrap();

                    if tail_segment.ident.to_string() != "Poll" {
                        return syn::Error::new(
                            output.span(),
                            "polling method must return a Poll value",
                        )
                        .to_compile_error()
                        .into();
                    }

                    let args = &tail_segment.arguments;

                    match *args {
                        syn::PathArguments::AngleBracketed(ref generic) => {
                            match generic.args.first().unwrap() {
                                syn::GenericArgument::Type(ty) => ty,
                                _ => {
                                    return syn::Error::new(
                                        tail_segment.span(),
                                        "Unrecognized tail segment",
                                    )
                                    .to_compile_error()
                                    .into();
                                }
                            }
                        }
                        ref args => {
                            return syn::Error::new(
                                    args.span(),
                                    "Error finding Poll inner result type. You must use the normal Poll<TYPE> syntax.",
                                )
                                .to_compile_error()
                                .into();
                        }
                    }
                }
                _ => {
                    return syn::Error::new(
                        output.span(),
                        "polling method must return a Poll value",
                    )
                    .to_compile_error()
                    .into()
                }
            },
        };

        let future_ident = syn::Ident::new(
            &future_name,
            Span::call_site().resolved_at(trait_name.span()),
        );

        let future_definition = quote! {
            #[derive(Debug)]
            struct #future_ident<T: #trait_name> {
                this: T,
            }

            impl<T: #trait_name> ::core::future::Future for #future_ident<T> {
                type Output = #poll_ready_type;

                fn poll(self: ::core::pin::Pin<&mut Self>, cx: &mut ::core::task::Context<'_>) -> ::core::task::Poll<Self::Output> {
                    unsafe { self.get_unchecked_mut() }.this.#poll_method_ident(cx)
                }
            }
        };

        let async_method_definition = quote! {
            fn #async_method_ident(self) -> #future_ident<Self>
                where Self: Sized,
            {
                #future_ident { this: self }
            }
        }
        .into();

        let async_method_definition =
            parse_macro_input!(async_method_definition as TraitItemMethod);

        method_defs.push(async_method_definition);
        future_defs.push(future_definition);
    }

    parsed
        .items
        .extend(method_defs.into_iter().map(TraitItem::Method));

    let mut output = parsed.into_token_stream();
    output.extend(future_defs);

    output.into()
}
