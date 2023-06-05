use std::collections::HashSet;

use proc_macro2::{Ident, Span};
use quote::quote;
use syn::{parse::Parse, punctuated::Punctuated, Token};

#[proc_macro_attribute]
pub fn simple_enum(metadata: proc_macro::TokenStream, input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = proc_macro2::TokenStream::from(input);
    let meta = syn::parse_macro_input!(metadata as Args);
    if meta.vars.contains(&Ident::new("display", Span::call_site())) {
        quote!(
            #[derive(Debug, serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Hash, displaydoc::Display)]
            #input
        ).into() 
    } else {
        quote!(
            #[derive(Debug, serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
            #input
        ).into()
    }
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