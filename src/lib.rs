/*!
`polling-async-trait` is a library that creates async methods associated with
polling methods on your traits. It is similar to [`async-trait`], but where
`async-trait` works on `async` methods, `polling-async-trait` works on `poll_`
methods.

# Usage

The entry point to this library is the [`async_poll_trait`][macro@async_poll_trait]
attribute. When applied to a trait, it scans the trait for each method tagged
with `async_method`. It treats each of these methods as an async polling
method, and for each one, it adds an equivalent async method to the trait.

```
# use std::task::{Context, Poll};
# use std::pin::Pin;
use polling_async_trait::async_poll_trait;
use std::io;

#[async_poll_trait]
trait ExampleTrait {
    // This will create an async method called `basic` on this trait
    #[async_method]
    fn poll_basic(&mut self, cx: &mut Context<'_>) -> Poll<i32>;

    // polling methods can also accept &self or Pin<&mut Self>
    #[async_method]
    fn poll_ref_method(&self, cx: &mut Context<'_>) -> Poll<i32>;

    #[async_method]
    fn poll_pin_method(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<i32>;

    // If `owned` is given, the generated async method will take `self` by move.
    // This means that the returned future will take ownership of this instance.
    // Owning futures can still be used with any of `&self`, `&mut self`, or
    // `Pin<&mut Self>`
    #[async_method(owned)]
    fn poll_close(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>>;

    #[async_method(owned)]
    fn poll_close_ref(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>>;

    #[async_method(owned)]
    fn poll_close_pinned(self: Pin<&mut Self>, cx: &mut Context<'_>)
        -> Poll<io::Result<()>>;

    // you can use method_name and future_name to control the names of the
    // generated async method and associated future. This will generate an
    // async method called do_work, and an associated `Future` called `DoWork`
    #[async_method(method_name = "do_work", future_name = "DoWork")]
    fn poll_work(&mut self, cx: &mut Context<'_>) -> Poll<()>;
}

#[derive(Default)]
struct ExampleStruct {
    closed: bool,
}

impl ExampleTrait for ExampleStruct {
    fn poll_basic(&mut self, cx: &mut Context<'_>) -> Poll<i32> {
        Poll::Ready(10)
    }

    fn poll_ref_method(&self, cx: &mut Context<'_>) -> Poll<i32> {
        Poll::Ready(20)
    }

    fn poll_pin_method(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<i32> {
        Poll::Ready(30)
    }

    fn poll_close(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.closed {
            println!("closing...");
            self.closed = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            println!("closed!");
            Poll::Ready(Ok(()))
        }
    }

    fn poll_close_ref(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.closed {
            println!("Error, couldn't close...");
            Poll::Ready(Err(io::ErrorKind::Other.into()))
        } else {
            println!("closed!");
            Poll::Ready(Ok(()))
        }
    }

    fn poll_close_pinned(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if !this.closed {
            println!("closing...");
            this.closed = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            println!("closed!");
            Poll::Ready(Ok(()))
        }
    }

    fn poll_work(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        Poll::Ready(())
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut data1 = ExampleStruct::default();

    assert_eq!(data1.basic().await, 10);
    assert_eq!(data1.ref_method().await, 20);
    data1.do_work().await;
    data1.close().await?;

    let data2 = ExampleStruct::default();
    assert!(data2.close_ref().await.is_err());

    let mut data3 = Box::pin(ExampleStruct::default());
    assert_eq!(data3.as_mut().pin_method().await, 30);

    let data4 = ExampleStruct::default();

    // Soundness: we can can await this method directly because it takes
    // ownership of `data4`.
    data4.close_pinned().await?;

    Ok(())
}
```

The generated future types will share visibility with the trait (that is, they
will be `pub` if the trait is `pub`, `pub(crate)` if the trait is `pub(crate)`,
etc).

# Tradeoffs with [`async-trait`]

Consider carefully which library is best for your use case; polling methods are
often much more difficult to write (because they require manual state management
& dealing with `Pin`). If your control flow is complex, it's probably
preferable to use an `async fn` and [`async-trait`]. The advantage of
`polling-async-trait` is that the async methods it creates are 0-overhead,
because the returned futures call the poll methods directly. This means there's
no need to use a type-erased `Box<dyn Future ... >`.

[`async-trait`]: https://docs.rs/async-trait
*/

extern crate proc_macro;
use inflector::Inflector;
use proc_macro::TokenStream as RawTokenStream;
use proc_macro2::{Ident, Span};
use quote::{format_ident, quote, ToTokens};
use syn::{
    parse_macro_input, spanned::Spanned, AngleBracketedGenericArguments, Attribute, Lifetime, Meta,
    MetaList, MetaNameValue, NestedMeta, PatType, Path, ReturnType, Signature, TraitItem,
    TraitItemMethod, Type, TypePath,
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum AsyncMethodType {
    Ref,
    Owned,
}

#[derive(Debug, Clone)]
struct MethodMeta {
    ty: AsyncMethodType,
    future_name: Option<String>,
    async_method_name: Option<String>,
}

#[derive(Debug, Copy, Clone)]
enum PollMethodReceiverType {
    Ref,
    MutRef,
    Pinned,
}

/// Given a return type matching `task::Poll<Type>`, extract `Type` (or return
/// an error)
fn extract_output_type(ret: &ReturnType) -> Result<&Type, RawTokenStream> {
    match *ret {
        syn::ReturnType::Type(_, ref ty) => match **ty {
            syn::Type::Path(ref path) => {
                let tail_segment = path.path.segments.last().unwrap();

                if tail_segment.ident.to_string() != "Poll" {
                    return Err(syn::Error::new(
                        ret.span(),
                        "polling method must return a Poll value",
                    )
                    .to_compile_error()
                    .into());
                }

                let args = &tail_segment.arguments;

                match *args {
                    syn::PathArguments::AngleBracketed(AngleBracketedGenericArguments {
                        args: ref generics,
                        ..
                    }) if generics.len() != 1 => Err(syn::Error::new(
                        args.span(),
                        "Poll return type should have exactly 1 generic parameter",
                    )
                    .to_compile_error()
                    .into()),

                    syn::PathArguments::AngleBracketed(AngleBracketedGenericArguments {
                        args: ref generics,
                        ..
                    }) => match *generics.first().unwrap() {
                        syn::GenericArgument::Type(ref ty) => Ok(ty),
                        _ => Err(syn::Error::new(
                            args.span(),
                            "Error parsing generics of Poll type",
                        )
                        .to_compile_error()
                        .into()),
                    },

                    _ => Err(syn::Error::new(
                        ret.span(),
                        "Poll return type must include the <Output> type",
                    )
                    .to_compile_error()
                    .into()),
                }
            }
            _ => Err(
                syn::Error::new(ret.span(), "polling method must return a Poll value")
                    .to_compile_error()
                    .into(),
            ),
        },
        _ => Err(
            syn::Error::new(ret.span(), "polling method must return a Poll value")
                .to_compile_error()
                .into(),
        ),
    }
}

/// Given a function signature, determine the receiver type. Accepts &self,
/// &mut self, and self: Pin<&mut Self>.
fn extract_poll_self_type(sig: &Signature) -> Option<PollMethodReceiverType> {
    match *sig.inputs.first()? {
        syn::FnArg::Receiver(ref recv) => {
            if recv.reference.is_none() {
                None
            } else if recv.mutability.is_some() {
                Some(PollMethodReceiverType::MutRef)
            } else {
                Some(PollMethodReceiverType::Ref)
            }
        }
        syn::FnArg::Typed(PatType {
            ref pat, ref ty, ..
        }) => {
            // Check that pattern is `self`
            let pat_ident = match **pat {
                syn::Pat::Ident(ref pat_ident) => pat_ident,
                _ => return None,
            };

            if pat_ident.by_ref.is_some() || pat_ident.subpat.is_some() {
                return None;
            }

            if pat_ident.ident != "self" {
                return None;
            }

            // Check that the type is Pin<&mut Self>
            let ty = match **ty {
                Type::Path(TypePath {
                    qself: None,
                    path: Path { ref segments, .. },
                }) => segments.last()?,
                _ => return None,
            };

            if ty.ident != "Pin" {
                return None;
            }

            let generics = match ty.arguments {
                syn::PathArguments::AngleBracketed(ref generics) => &generics.args,
                _ => return None,
            };

            if generics.len() != 1 {
                return None;
            }

            let ty = match generics.first()? {
                syn::GenericArgument::Type(Type::Reference(ty)) => ty,
                _ => return None,
            };

            if ty.mutability.is_none() {
                return None;
            }

            let self_ident = match *ty.elem {
                Type::Path(TypePath {
                    qself: None,
                    ref path,
                }) => path.get_ident()?,
                _ => return None,
            };

            if self_ident != "Self" {
                return None;
            }

            Some(PollMethodReceiverType::Pinned)
        }
    }
}

/// Given a list of attributes on a method, if it has an async_method, parse
/// and remove it
fn extract_meta<'a>(attrs: &'a mut Vec<Attribute>) -> Option<Result<MethodMeta, RawTokenStream>> {
    for (index, attr) in attrs.iter_mut().enumerate() {
        let meta = match attr.parse_meta() {
            Ok(meta) => meta,
            Err(..) => continue,
        };

        let (path, nested) = match meta {
            syn::Meta::Path(path) => (path, None),
            syn::Meta::List(MetaList { path, nested, .. }) => (path, Some(nested)),
            _ => continue,
        };

        match path.get_ident() {
            Some(ident) if ident == "async_method" => {}
            _ => continue,
        }

        // At this point, we know we have an async_method. Anything wrong past this
        // point should result in an error.

        attrs.remove(index);

        let mut result = MethodMeta {
            ty: AsyncMethodType::Ref,
            async_method_name: None,
            future_name: None,
        };

        if let Some(meta_args) = nested {
            for arg in meta_args.iter() {
                match arg {
                    NestedMeta::Meta(Meta::NameValue(MetaNameValue {
                        path,
                        lit: syn::Lit::Str(name),
                        ..
                    })) => {
                        let ident = match path.get_ident() {
                            Some(ident) => ident,
                            None => {
                                return Some(Err(syn::Error::new(
                                    path.span(),
                                    "Unrecognized meta argument",
                                )
                                .to_compile_error()
                                .into()))
                            }
                        };

                        if ident == "method_name" {
                            result.async_method_name = Some(name.value())
                        } else if ident == "future_name" {
                            result.future_name = Some(name.value())
                        } else {
                            return Some(Err(syn::Error::new(
                                path.span(),
                                "Unrecognized meta argument",
                            )
                            .to_compile_error()
                            .into()));
                        }
                    }
                    NestedMeta::Meta(Meta::Path(path)) => {
                        let ident = match path.get_ident() {
                            Some(ident) => ident,
                            None => {
                                return Some(Err(syn::Error::new(
                                    path.span(),
                                    "Unrecognized meta argument",
                                )
                                .to_compile_error()
                                .into()))
                            }
                        };

                        if ident == "owned" {
                            result.ty = AsyncMethodType::Owned;
                        } else {
                            return Some(Err(syn::Error::new(
                                path.span(),
                                "Unrecognized meta argument",
                            )
                            .to_compile_error()
                            .into()));
                        }
                    }
                    _ => {
                        return Some(Err(syn::Error::new(
                            arg.span(),
                            "Unrecognized meta argument",
                        )
                        .to_compile_error()
                        .into()))
                    }
                }
            }
        }

        return Some(Ok(result));
    }

    None
}

#[proc_macro_attribute]
pub fn async_poll_trait(_attr: RawTokenStream, item: RawTokenStream) -> RawTokenStream {
    let mut parsed = parse_macro_input!(item as syn::ItemTrait);

    let trait_ident = &parsed.ident;
    let trait_name = trait_ident.to_string();
    let vis = &parsed.vis;

    let mut new_methods = Vec::new();
    let mut new_structs = Vec::new();

    for item in &mut parsed.items {
        // Is this a method?
        let method = match item {
            TraitItem::Method(method) => method,
            _ => continue,
        };

        // Check if this method should be async'd
        let meta = match extract_meta(&mut method.attrs) {
            None => continue,
            Some(Err(err)) => return err,
            Some(Ok(meta)) => meta,
        };

        // We have a meta, so we know this method has been designated to
        // by processed by this library. Anything that fails at this point
        // is an error.

        // Get the return type our future will use
        let output_type = match extract_output_type(&method.sig.output) {
            Ok(ty) => ty,
            Err(err) => return err,
        };

        // Check what kind of receiver this method uses (&self, &mut self, self: Pin<&mut Self>)
        let receiver_type =
            match extract_poll_self_type(&method.sig) {
                Some(receiver_type) => receiver_type,
                None => return syn::Error::new(
                    method.sig.span(),
                    "poll function must be a method that takes &self, &mut self, or Pin<&mut Self>",
                )
                .to_compile_error()
                .into(),
            };

        let poll_method_ident = &method.sig.ident;
        let poll_method_name = poll_method_ident.to_string();

        // poll_base_name => base_name
        let base_name = poll_method_name.strip_prefix("poll_");

        let async_method_name = match meta.async_method_name.as_deref().or(base_name) {
            Some(name) => name,
            None => {
                return syn::Error::new(
                    poll_method_ident.span(),
                    "poll method must start with poll_",
                )
                .to_compile_error()
                .into()
            }
        };
        let async_method_ident = Ident::new(
            async_method_name,
            Span::call_site().resolved_at(poll_method_ident.span()),
        );

        let future_name = match meta
            .future_name
            .or_else(|| base_name.map(|name| format!("{}{}", trait_name, name.to_class_case())))
        {
            Some(name) => name,
            None => {
                return syn::Error::new(
                    poll_method_ident.span(),
                    "poll method must start with poll_",
                )
                .to_compile_error()
                .into()
            }
        };

        let future_ident = Ident::new(
            future_name.as_str(),
            Span::call_site().resolved_at(trait_ident.span()),
        );

        // That's everything we need; now it's just a matter of constructing
        // the new methods and new future structs and inserting them in the
        // right places.

        // These will come in handy later. They allow us to stitch together
        // several quotes!() and make sure the identifier hygiene lines up.
        let self_ident = format_ident!("self");
        let cx_ident = format_ident!("cx");
        let inner_ident = format_ident!("inner");
        let generic_ident = format_ident!("T");
        let generic_lt = Lifetime::new("'a", Span::call_site());

        let (async_def, future_def) = match meta.ty {
            AsyncMethodType::Owned => {
                let async_method_definition = quote! {
                    fn #async_method_ident(self) -> #future_ident<Self>
                        where Self: Sized
                    {
                        #future_ident { #inner_ident: self }
                    }
                };

                // Safety of this definition:
                // - if receiver type is ref or mut ref, we can ignore the
                //   pin entirely (project to unpin)
                // - if receiver type is pin, we know that self is pinned, so
                //   it's safe to project to an inner pin
                // We could do the same thing with pin_project, and avoid
                // unsafe, but we'd rather avoid the dependency for something
                // so simple

                let future_poll_definition = match receiver_type {
                    PollMethodReceiverType::MutRef => quote! {
                        unsafe { #self_ident.get_unchecked_mut() }.#inner_ident.#poll_method_ident(#cx_ident)
                    },
                    PollMethodReceiverType::Ref => quote! {
                        #self_ident.into_ref().get_ref().#inner_ident.#poll_method_ident(#cx_ident)
                    },
                    PollMethodReceiverType::Pinned => quote! {
                        unsafe { Pin::new_unchecked(&mut #self_ident.get_unchecked_mut().#inner_ident) }.#poll_method_ident(#cx_ident)
                    },
                };

                let future_definition = quote! {
                    #[derive(Debug)]
                    #vis struct #future_ident<T: #trait_ident> {
                        #inner_ident: T,
                    }

                    impl<T: #trait_ident> ::core::future::Future for #future_ident<T> {
                        type Output = #output_type;

                        fn poll(
                            #self_ident: ::core::pin::Pin<&mut Self>,
                            #cx_ident: &mut ::core::task::Context<'_>,
                        ) -> ::core::task::Poll<Self::Output>
                        {
                            #future_poll_definition
                        }
                    }
                };

                (async_method_definition, future_definition)
            }
            AsyncMethodType::Ref => {
                let async_method_receiver = match receiver_type {
                    PollMethodReceiverType::Ref => quote! { &#self_ident },
                    PollMethodReceiverType::MutRef => quote! { &mut #self_ident },
                    PollMethodReceiverType::Pinned => {
                        quote! { #self_ident: ::core::pin::Pin<&mut Self> }
                    }
                };

                let async_method_definition = quote! {
                    fn #async_method_ident(#async_method_receiver) -> #future_ident<Self> {
                        #future_ident { #inner_ident: #self_ident }
                    }
                };

                let future_inner_type = match receiver_type {
                    PollMethodReceiverType::Ref => quote! {& #generic_lt #generic_ident },
                    PollMethodReceiverType::MutRef => quote! { & #generic_lt mut #generic_ident },
                    PollMethodReceiverType::Pinned => {
                        quote! { Pin<& #generic_lt mut #generic_ident> }
                    }
                };

                let future_poll_definition = match receiver_type {
                    PollMethodReceiverType::Ref | PollMethodReceiverType::MutRef => quote! {
                        #self_ident.get_mut().#inner_ident.#poll_method_ident(#cx_ident)
                    },
                    PollMethodReceiverType::Pinned => quote! {
                        #self_ident.get_mut().#inner_ident.as_mut().#poll_method_ident(#cx_ident)
                    },
                };

                let future_definition = quote! {
                    #[derive(Debug)]
                    #vis struct #future_ident<#generic_lt, #generic_ident: #trait_ident + ?Sized> {
                        #inner_ident: #future_inner_type,
                    }

                    impl<'a, T: #trait_ident + ?Sized> ::core::future::Future for #future_ident<'a, T> {
                        type Output = #output_type;

                        fn poll(
                            #self_ident: ::core::pin::Pin<&mut Self>,
                            #cx_ident: &mut ::core::task::Context<'_>,
                        ) -> ::core::task::Poll<Self::Output>
                        {
                            #future_poll_definition
                        }
                    }
                };

                (async_method_definition, future_definition)
            }
        };

        let async_def = async_def.into();
        let async_def = parse_macro_input!(async_def as TraitItemMethod);

        new_methods.push(async_def);
        new_structs.push(future_def);
    }

    // Add the new methods to the trait
    parsed
        .items
        .extend(new_methods.into_iter().map(TraitItem::Method));

    let mut output = parsed.into_token_stream();

    // Add the new future definitions to the output
    output.extend(new_structs);

    output.into()
}
