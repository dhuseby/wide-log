use proc_macro::TokenStream;

#[proc_macro]
pub fn wide_log(_input: TokenStream) -> TokenStream {
    TokenStream::new()
}