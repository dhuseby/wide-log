use proc_macro2::{Span, TokenStream as TokenStream2};

mod codegen;
mod parse;

#[proc_macro]
pub fn wide_log(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input2 = TokenStream2::from(input);
    let (overrides, node) = match parse::parse_wide_log_input(&input2) {
        Ok((o, n)) => (o, n),
        Err(e) => {
            return syn::Error::new(Span::call_site(), e)
                .to_compile_error()
                .into();
        }
    };
    let tokio = cfg!(feature = "tokio");
    let uuid = cfg!(feature = "uuid");
    let tracing = cfg!(feature = "tracing");
    match codegen::generate(node, overrides, tokio, uuid, tracing) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}
