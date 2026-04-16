//! Proc-macro backing for the `hsmc` crate. The sole public item is the
//! [`statechart!`] macro which generates a state machine from a declarative
//! description.

use proc_macro::TokenStream;

mod parse;
mod ir;
mod validate;
mod codegen;

/// See the `hsmc` crate for documentation.
#[proc_macro]
pub fn statechart(input: TokenStream) -> TokenStream {
    let input = proc_macro2::TokenStream::from(input);
    match statechart_impl(input) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn statechart_impl(input: proc_macro2::TokenStream) -> syn::Result<proc_macro2::TokenStream> {
    let parsed = syn::parse2::<parse::StatechartInput>(input)?;
    validate::validate_parse_tree(&parsed)?;
    let mut ir = ir::build_ir(parsed)?;
    ir::resolve_transitions(&mut ir)?;
    validate::validate(&ir)?;
    Ok(codegen::generate(&ir))
}
