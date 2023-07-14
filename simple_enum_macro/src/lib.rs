use std::collections::HashSet;

use proc_macro2::{Ident, Span};
use quote::quote;
use syn::{parse::Parse, punctuated::Punctuated, Token};

/// Helper macro for deriving many traits at once, principally for C-style enums.
/// Specifically, it derives `Debug`, `serde::Serialize`, `serde::Deserialize`, `Clone`, `Copy`, `PartialEq`, `Eq`, 
/// `Hash`, and optionally `displaydoc::Display`.
/// 
/// `Copy` can be excluded by using the `no_copy` argument, and `Display` can be included with 
/// `display`. 
/// 
/// ## Usage:
/// ```rust
/// #[simple_enum]
/// enum MyEnum {}
/// 
/// #[simple_enum(display)]
/// enum MyDisplayEnum {
///     /// Variant
///     Variant,
///     /// Another Variant ({0})
///     AnotherVariant(i32),
/// }
/// 
/// #[simple_enum(display, no_copy)]
/// enum MyNoCopyEnum {
///     /// Data: {0}
///     SomeData(String),
/// }
/// ```
#[proc_macro_attribute]
pub fn simple_enum(metadata: proc_macro::TokenStream, input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = proc_macro2::TokenStream::from(input);
    let meta = syn::parse_macro_input!(metadata as Args);
    let mut traits = vec![
        quote!(Debug),
        quote!(serde::Serialize),
        quote!(serde::Deserialize),
        quote!(Clone),
        quote!(PartialEq),
        quote!(Eq),
        quote!(Hash),
    ];
    if meta.vars.contains(&Ident::new("display", Span::call_site())) {
        traits.push(quote!(displaydoc::Display));
    }
    if !meta.vars.contains(&Ident::new("no_copy", Span::call_site())) {
        traits.push(quote!(Copy));
    }
    quote!(
        #[derive(#(#traits),*)]
        #input
    ).into()
}

struct Args {
    vars: HashSet<Ident>,
}

impl Parse for Args {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let vars = Punctuated::<Ident, Token![,]>::parse_terminated(input)?;
        Ok(Args {
            vars: vars.into_iter().collect(),
        })
    }
}