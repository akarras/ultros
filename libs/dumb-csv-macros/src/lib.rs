use std::str::FromStr;

use darling::{FromDeriveInput, FromField, ast};
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::{DeriveInput, parse_macro_input};

#[proc_macro_derive(DumbCsvDeserialize, attributes(dumb_csv))]
pub fn dumb_deserialize(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let receiver =
        DumbCsvDeserializeReceiver::from_derive_input(&input).expect("This to be derive input");
    quote! {
        // #input

        #receiver
    }
    .into()
}

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(dumb_csv), supports(struct_any))]
struct DumbCsvDeserializeReceiver {
    ident: syn::Ident,
    generics: syn::Generics,
    data: ast::Data<(), DumbFieldReceiver>,
}

#[derive(Debug, PartialEq)]
enum DummyType<'a> {
    String,
    Bool,
    Other(&'a str),
}

impl<'a> From<&'a str> for DummyType<'a> {
    fn from(value: &'a str) -> Self {
        let value = value
            .trim_start_matches("Vec <")
            .trim_end_matches(">")
            .trim();
        match value {
            "String" => Self::String,
            "bool" => Self::Bool,
            t => Self::Other(t),
        }
    }
}

impl ToTokens for DumbCsvDeserializeReceiver {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let DumbCsvDeserializeReceiver {
            ident,
            generics,
            data,
        } = self;
        let (imp, ty, wher) = generics.split_for_impl();
        let fields = data
            .as_ref()
            .take_struct()
            .expect("Should never be a struct")
            .fields;
        let fields = fields
            .iter()
            .map(|field| {
                let field_name = field.ident.as_ref().expect("Only supports named fields");
                let ty = &field.ty;
                let d = ty.into_token_stream().to_string();
                let dummy: DummyType = d.as_str().into();
                let parse_body = match dummy {
                    DummyType::String => quote! {},
                    DummyType::Bool => quote! { .parse_bool() },
                    DummyType::Other(val) => {
                        let ty= TokenStream::from_str(val).unwrap();
                        // let ty = Type::from(val);
                        if val.starts_with("i") || val.starts_with("u") || val.ends_with("Id") {
                            quote!{
                                .parse::<#ty>().unwrap_or_default()
                            }
                        } else {
                            let error = format!("DUMBCSV OTHER {val}: Error parsing value {}", field_name);
                            quote!{
                                .parse::<#ty>().expect(#error)
                            }
                        }
                    }
                };
                let mut parser = if let Some(count) = field.count {
                    // let error = format!("Error reading value {}  dummy {dummy:?}: {}", field_name, ty.to_token_stream().to_string());
                    quote! {
                        list.by_ref().take(#count).map(|l| {
                            println!("{l}");
                            l #parse_body
                        }).collect()
                    }
                } else {
                    match dummy { DummyType::String => {
                        let error = format!("Error reading value {}", field_name);
                        quote! {
                            list.next().expect(#error).to_string()
                        }
                        }, DummyType::Bool => {
                            quote! {
                                list.next().expect("There to be a value").parse_bool()
                            }
                        }, DummyType::Other(val) => {
                            if val.starts_with("i") || val.starts_with("u") || val.ends_with("Id") {
                                quote!{
                                    list.next().expect("There to be a value").parse::<#ty>().unwrap_or_default()
                                }
                            } else {
                                let error = format!("DUMBCSV OTHER {val}: Error parsing value {}", field_name);
                                quote!{
                                    list.next().expect("There to be a value").parse::<#ty>().expect(#error)
                                }
                            }
                        },
                    }
                };
                if let Some(skip) = field.skip {
                    parser = quote! {
                        {
                            let value = #parser;
                            for _ in list.by_ref().take(#skip) {

                            }
                            value
                        }
                    };
                }
                quote! {
                    #field_name: #parser,
                }
            })
            .collect::<TokenStream>();
        let output_tokens = quote! {
            impl #imp DumbCsvDeserialize for #ident #ty #wher {
                fn from_str_list<'a>(mut list: impl Iterator<Item = &'a str>) -> Self {
                    Self {
                        #fields
                    }
                }
            }
        };
        // panic!("{output_tokens}");
        tokens.extend(output_tokens);
    }
}

#[derive(Debug, FromField)]
#[darling(attributes(dumb_csv))]
struct DumbFieldReceiver {
    /// Get the ident of the field. For fields in tuple or newtype structs or
    /// enum bodies, this can be `None`.
    ident: Option<syn::Ident>,

    /// This magic field name pulls the type from the input.
    ty: syn::Type,

    /// How many fields a vec should collect
    count: Option<usize>,

    /// How many fields to skip after parsing this
    skip: Option<usize>,
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
