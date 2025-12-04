use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Attribute, Data, DeriveInput, Fields, Meta};

fn extract_doc_lines(attrs: &[Attribute]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("doc") {
            if let Meta::NameValue(nv) = &attr.meta {
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s), ..
                }) = &nv.value
                {
                    let text = s.value();
                    let trimmed = text.strip_prefix(' ').unwrap_or(&text);
                    lines.push(trimmed.to_string());
                }
            }
        }
    }
    lines.join("\n")
}

fn generate_doc_impl(name: &syn::Ident, self_doc: &str, members: &[(String, String)]) -> TokenStream2 {
    let mod_name = format_ident!("{}_doc_str_impl", name);
    let self_static = format_ident!("__Self_{}_doc_str", name);

    let static_defs: Vec<TokenStream2> = {
        let mut defs = vec![quote! {
            #[allow(non_upper_case_globals)]
            pub static #self_static: &str = #self_doc;
        }];
        for (field_name, doc) in members {
            let static_ident = format_ident!("{}_doc_str", field_name);
            defs.push(quote! {
            #[allow(non_upper_case_globals)]
                pub static #static_ident: &str = #doc;
            });
        }
        defs
    };

    let getters: Vec<TokenStream2> = {
        let mut methods = vec![quote! {
            #[inline(always)]
            pub const fn doc_str(&self) -> &'static str {
                #mod_name::#self_static
            }

            #[inline(always)]
            pub const fn doc_str_static() -> &'static str {
                #mod_name::#self_static
            }

        }];
        for (field_name, _doc) in members {
            let method_name = format_ident!("{}_doc_str", field_name);
            let method_name_static = format_ident!("{}_doc_str_static", field_name);
            let static_ident = format_ident!("{}_doc_str", field_name);
            methods.push(quote! {
                #[inline(always)]
                pub const fn #method_name(&self) -> &'static str {
                    #mod_name::#static_ident
                }
                #[inline(always)]
                pub const fn #method_name_static() -> &'static str {
                    #mod_name::#static_ident
                }
            });
        }
        methods
    };

    quote! {
        #[doc(hidden)]
        #[allow(non_snake_case, dead_code, non_upper_case_globals)]
        mod #mod_name {
            #(#static_defs)*
        }

        impl #name {
            #(#getters)*
        }
    }
}

fn generate_enum_variant_getters(name: &syn::Ident, variants: &[(String, String)]) -> TokenStream2 {
    let mod_name = format_ident!("{}_doc_str_impl", name);

    let methods: Vec<TokenStream2> = variants
        .iter()
        .map(|(variant_name, _doc)| {
            let method_name = format_ident!("{}_doc_str", variant_name);
            let method_name_static = format_ident!("{}_doc_str_static", variant_name);
            let static_ident = format_ident!("{}_doc_str", variant_name);
            quote! {
                #[inline(always)]
                pub fn #method_name(&self) -> &'static str {
                    #mod_name::#static_ident
                }
                #[inline(always)]
                pub fn #method_name_static() -> &'static str {
                    #mod_name::#static_ident
                }
            }
        })
        .collect();

    quote! {
        impl #name {
            #(#methods)*
        }
    }
}

#[proc_macro_attribute]
pub fn with_doc_str(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let name = &input.ident;
    let self_doc = extract_doc_lines(&input.attrs);

    let expanded = match &input.data {
        Data::Struct(data_struct) => {
            let members: Vec<(String, String)> = match &data_struct.fields {
                Fields::Named(fields) => fields
                    .named
                    .iter()
                    .filter_map(|f| {
                        let fname = f.ident.as_ref()?.to_string();
                        let doc = extract_doc_lines(&f.attrs);
                        Some((fname, doc))
                    })
                    .collect(),
                Fields::Unnamed(fields) => fields
                    .unnamed
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        let fname = format!("{}", i);
                        let doc = extract_doc_lines(&f.attrs);
                        (fname, doc)
                    })
                    .collect(),
                Fields::Unit => vec![],
            };

            let doc_impl = generate_doc_impl(name, &self_doc, &members);
            quote! { #doc_impl }
        }

        Data::Enum(data_enum) => {
            let members: Vec<(String, String)> = data_enum
                .variants
                .iter()
                .map(|v| {
                    let vname = v.ident.to_string();
                    let doc = extract_doc_lines(&v.attrs);
                    (vname, doc)
                })
                .collect();

            let doc_impl = generate_doc_impl(name, &self_doc, &members);
            let variant_getters = generate_enum_variant_getters(name, &members);
            quote! {
                #doc_impl
                #variant_getters
            }
        }

        Data::Union(_) => {
            return syn::Error::new_spanned(&input, "with_doc_str does not support unions")
                .to_compile_error()
                .into();
        }
    };

    let original = quote! { #input };
    let output = quote! {
        #original
        #expanded
    };

    output.into()
}
