use darling::{ast, FromDeriveInput, FromField};
use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::{parse_macro_input, DeriveInput, Ident, Type};

#[proc_macro_derive(FromCsv, attributes(xiv_gen))]
pub fn derive_from_csv(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let receiver = FromCsvReceiver::from_derive_input(&input).expect("Failed to parse derive input");

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

        let field_parsers = fields.iter().map(|f| {
            let field_ident = f.ident.as_ref().unwrap();
            let ty = &f.ty;
            let col_name = f.column.as_ref().cloned().unwrap_or_else(|| {
                // convert snake_case to CamelCase if needed?
                // Actually ffxiv-datamining headers are usually CamelCase.
                // But let's assume the user provides it if it's different.
                field_ident.to_string()
            });

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
                            vec.push(val.parse().unwrap_or_else(|_| panic!("Failed to parse {} at index {}", stringify!(#field_ident), idx)));
                        }
                        vec.try_into().unwrap_or_else(|_| panic!("Failed to convert Vec to array for {}", stringify!(#field_ident)))
                    }
                }
            } else {
                // Single field
                quote! {
                    #field_ident: {
                        let idx = header.iter().position(|h| h == #col_name).expect(&format!("Column {} not found", #col_name));
                        let val = row.get(idx).unwrap_or_default();
                        val.parse().unwrap_or_else(|_| panic!("Failed to parse {} with value '{}'", #col_name, val))
                    }
                }
            }
        });

        tokens.extend(quote! {
            impl #ident {
                pub fn from_csv_row(header: &[String], row: &csv::StringRecord) -> Self {
                    Self {
                        #( #field_parsers ),*
                    }
                }
            }
        });
    }
}
