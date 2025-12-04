use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, parse_quote, Data, DeriveInput, Fields};

#[proc_macro_derive(WithSelfSanitize, attributes(sanitize_inplace, sanitize_inplace_with_epilogue))]
pub fn derive_sanitize_inplace(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match expand(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;

    let trait_path: syn::Path = parse_quote!(WithSelfSanitize);

    let needs_epilogue = input
        .attrs
        .iter()
        .find(|a| a.path().is_ident("sanitize_inplace_with_epilogue"))
        .is_some();

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "WithSelfSanitize only supports structs with named fields",
                ))
            }
        },
        _ => return Err(syn::Error::new_spanned(name, "WithSelfSanitize only supports structs")),
    };

    let mut calls = Vec::new();

    for field in fields {
        if is_skipped(field)? {
            continue;
        }

        let ident = field
            .ident
            .as_ref()
            .ok_or_else(|| syn::Error::new_spanned(field, "only named fields are supported"))?;

        calls.push(quote! {
            self.#ident.sanitize_inplace(());
        });
    }

    // let (impl_generics, ty_generics, where_clause) =
    //     input.generics.split_for_impl();
    let mut generics = input.generics.clone();
    for param in generics.type_params_mut() {
        param.bounds.push(parse_quote!(#trait_path));
    }

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let epilogue = if needs_epilogue {
        quote!(self.sanitize_inplace_epilogue();)
    } else {
        quote!()
    };

    Ok(quote! {
        impl #impl_generics #trait_path for #name #ty_generics #where_clause {
            type SanInputT = ();
            fn sanitize_inplace(&mut self, #[allow(unused)]input: Self::SanInputT) {
                #(#calls)*
                #epilogue
            }
        }
    })
}

fn is_skipped(field: &syn::Field) -> syn::Result<bool> {
    for attr in &field.attrs {
        if !attr.path().is_ident("sanitize_inplace") {
            continue;
        }

        if matches!(attr.meta, syn::Meta::Path(_)) {
            continue;
        }

        let mut skip = false;

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                skip = true;
                Ok(())
            } else {
                Err(meta.error("expected `sanitize_inplace(skip)`"))
            }
        })?;

        if skip {
            return Ok(true);
        }
    }

    Ok(false)
}
