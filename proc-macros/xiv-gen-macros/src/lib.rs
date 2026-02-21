use darling::{FromDeriveInput, FromField, ast};
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::{DeriveInput, Ident, Type, parse_macro_input};

#[proc_macro_derive(FromCsv, attributes(xiv_gen))]
pub fn derive_from_csv(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let receiver =
        FromCsvReceiver::from_derive_input(&input).expect("Failed to parse derive input");

    quote! {
        #receiver
    }
    .into()
}

#[derive(Debug, FromDeriveInput)]
#[darling(attributes(xiv_gen), supports(struct_named))]
struct FromCsvReceiver {
    ident: Ident,
    data: ast::Data<(), FromCsvFieldReceiver>,
    #[darling(default)]
    sheet: Option<String>,
}

#[derive(Debug, FromField)]
#[darling(attributes(xiv_gen))]
struct FromCsvFieldReceiver {
    ident: Option<Ident>,
    ty: Type,
    #[darling(default)]
    column: Option<String>,
    #[darling(default)]
    count: Option<usize>,
}

impl ToTokens for FromCsvReceiver {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let ident = &self.ident;
        let fields = self.data.as_ref().take_struct().unwrap().fields;
        let sheet = self
            .sheet
            .as_ref()
            .map(|s| quote! { Some(#s) })
            .unwrap_or(quote! { None });

        let field_parsers = fields.iter().map(|f| {
            let field_ident = f.ident.as_ref().unwrap();
            let ty = &f.ty;
            let col_name = f
                .column
                .as_ref()
                .cloned()
                .unwrap_or_else(|| field_ident.to_string());

            if let Some(count) = f.count {
                // Array field
                let indices = (0..count).map(|i| {
                    let c = col_name.replace("{}", &i.to_string());
                    quote! {
                        header.iter().position(|h| h == #c).expect(&format!("Column {} not found", #c))
                    }
                });
                quote! {
                    #field_ident: {
                        let indices = [#( #indices ),*];
                        let mut vec = Vec::with_capacity(#count);
                        for idx in indices {
                            let val = row.get(idx).unwrap_or_default();
                            vec.push(val.parse().unwrap_or_else(|_| panic!("Failed to parse {} at index {} with value '{}' as {}", stringify!(#field_ident), idx, val, stringify!(#ty))));
                        }
                        vec.try_into().unwrap_or_else(|_| panic!("Failed to convert Vec to array for {}", stringify!(#field_ident)))
                    }
                }
            } else {
                // Single field
                let is_bool = f.ty.to_token_stream().to_string() == "bool";
                let parser = if is_bool {
                    quote! {
                        match val.to_lowercase().as_str() {
                            "true" | "1" => true,
                            _ => false,
                        }
                    }
                } else {
                    quote! {
                        val.parse().unwrap_or_else(|_| panic!("Failed to parse {} with value '{}' as {}", #col_name, val, stringify!(#ty)))
                    }
                };

                quote! {
                    #field_ident: {
                        let idx = header.iter().position(|h| h == #col_name).expect(&format!("Column {} not found in header. Available columns: {:?}", #col_name, header));
                        let val = row.get(idx).unwrap_or_default();
                        #parser
                    }
                }
            }
        });

        tokens.extend(quote! {
            impl crate::FromCsv for #ident {
                fn from_csv_row(header: &[String], row: &csv::StringRecord) -> Self {
                    Self {
                        #( #field_parsers ),*
                    }
                }
            }
            impl #ident {
                pub const SHEET: Option<&'static str> = #sheet;
            }
        });
    }
}
