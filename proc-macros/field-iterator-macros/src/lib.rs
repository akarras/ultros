use darling::{
    Error, FromDeriveInput, FromField, FromMeta,
    ast::{self, NestedMeta},
};
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::{DeriveInput, FnArg, ItemFn, parse_macro_input};

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(label_iterator), supports(struct_any))]
struct FieldLabelIterator {
    ident: syn::Ident,
    generics: syn::Generics,
    data: ast::Data<(), FieldTraitReceiver>,
}

impl ToTokens for FieldLabelIterator {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let Self {
            ident,
            generics,
            data,
        } = self;
        let (imp, ty, wher) = generics.split_for_impl();
        let fields = data
            .as_ref()
            .take_struct()
            .expect("Should be struct")
            .fields
            .iter()
            .map(|field| {
                let field_str = field.ident.as_ref().unwrap().to_string();
                quote! {
                    #field_str,
                }
            })
            .collect::<TokenStream>();
        let trait_impl = quote! {
            impl #imp FieldLabels for #ident #ty #wher {
                fn field_labels() -> &'static [&'static str] {
                    &[#fields]
                }
            }
        };
        // panic!("{trait_impl}");
        tokens.extend(trait_impl)
    }
}

#[proc_macro_derive(FieldLabels, attributes(field_labels))]
pub fn field_label_iterator(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let receiver = FieldLabelIterator::from_derive_input(&input).expect("This to be derive input");
    quote! {
        #receiver
    }
    .into()
}

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(sortable_vec), supports(struct_any))]
struct SortableVecReceiver {
    ident: syn::Ident,
    generics: syn::Generics,
    data: ast::Data<(), FieldTraitReceiver>,
}

impl ToTokens for SortableVecReceiver {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let Self {
            ident,
            generics,
            data,
        } = self;
        let (imp, ty, wher) = generics.split_for_impl();
        let matches = data
            .as_ref()
            .take_struct()
            .expect("Should be struct")
            .fields
            .iter()
            .map(|field| {
                let field_ident = field.ident.as_ref().unwrap();
                let field_str = field_ident.to_string();
                quote! {
                    #field_str => b.#field_ident.cmp(&a.#field_ident),
                }
            })
            .collect::<TokenStream>();
        let trait_impl = quote! {
            impl #imp SortableVec for #ident #ty #wher {
                fn sort_vec_by_label(vec: &mut Vec<#ident>, field_label: &str, then_by: Option<&str>) {
                    vec.sort_by(|a, b| {
                        let mut ord = match field_label {
                            #matches
                            _ => panic!("Unsupported field type"),
                        };
                        if let Some(then) = then_by {
                            ord = ord.then_with(|| match then {
                                #matches
                                _ => panic!("Unsupported field type"),
                            });
                        }
                        ord
                    });
                }
            }
        };
        // panic!("{trait_impl}");
        tokens.extend(trait_impl)
    }
}

#[derive(Debug, FromField)]
// #[darling(attributes(sortable_vec))]
struct FieldTraitReceiver {
    /// Get the ident of the field. For fields in tuple or newtype structs or
    /// enum bodies, this can be `None`.
    ident: Option<syn::Ident>,
}

#[proc_macro_derive(SortableVec, attributes(sortable_vec))]
pub fn field_trait_iterator(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let receiver = SortableVecReceiver::from_derive_input(&input).expect("This to be derive input");

    quote! {
        #receiver
    }
    .into()
}

#[derive(Debug, FromMeta)]
struct GenerateArgs {
    count: usize,
    field_prefix: String,
    field_postfix: Option<String>,
}

#[proc_macro_attribute]
pub fn field_iter(
    args: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let attr_args = match NestedMeta::parse_meta_list(args.into()) {
        Ok(v) => v,
        Err(e) => {
            return proc_macro::TokenStream::from(Error::from(e).write_errors());
        }
    };
    let GenerateArgs {
        count,
        field_postfix,
        field_prefix,
    } = match GenerateArgs::from_list(&attr_args) {
        Ok(v) => v,
        Err(e) => return proc_macro::TokenStream::from(e.write_errors()),
    };
    let function = parse_macro_input!(item as ItemFn);
    let function_name = function.sig.ident;
    let output = function.sig.output;
    let Some(first_arg) = function.sig.inputs.first() else {
        return proc_macro::TokenStream::from(
            Error::custom("No argument on function, should pass in parent struct as arg")
                .write_errors(),
        );
    };
    let item_identifier = match first_arg {
        FnArg::Receiver(_) => {
            return proc_macro::TokenStream::from(
                Error::custom("&self is first arg, when must be an input").write_errors(),
            );
        }
        FnArg::Typed(pattern) => &pattern.pat,
    };

    let fields: proc_macro2::TokenStream = (0..count)
        .map(|i| {
            let ident = if let Some(postfix) = &field_postfix {
                format_ident!("{field_prefix}{i}{postfix}")
            } else {
                format_ident!("{field_prefix}{i}")
            };
            quote! {
                &#item_identifier.#ident,
            }
        })
        .collect();

    let tokens = quote! {
        fn #function_name(#first_arg) #output {
            [#fields].into_iter().flatten().copied()
        }
    };
    // panic!("{tokens}");
    tokens.into()
}

#[cfg(test)]
mod tests {
    // use super::*;

    // struct SomeStruct {
    //     pub val_1_0: u32,
    //     pub val_2_0: u32,
    //     pub val_3_0: u32,
    //     pub val_1_1: u32,
    //     pub val_2_1: u32,
    //     pub val_3_1: u32,
    // }

    // struct Val1Iter;

    // // #[field_iter(count = "3", field_prefix = "val_", field_postfix = "0")]
    // // fn get_value_iterator(some_struct: &SomeStruct) -> impl Iterator<Item = u32> {
    // //     todo!()
    // // }

    // #[test]
    // fn it_works() {

    // }
}
