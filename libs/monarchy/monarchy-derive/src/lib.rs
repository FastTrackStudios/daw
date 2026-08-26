use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, Ident, Type};

/// Derive macro for the Metadata trait
///
/// This macro automatically generates:
/// - A Field enum with variants for each struct field
/// - A Value enum that can hold any of the field types
/// - The Metadata trait implementation
/// - GroupBuilder extension methods (like `.multi_mic()`, `.layers()`, etc.)
///
/// # Example
///
/// ```ignore
/// use monarchy::Metadata;
///
/// #[derive(Metadata, Clone, Default)]
/// struct AudioMetadata {
///     instrument: Option<String>,
///     performer: Option<String>,
///     multi_mic: Option<Vec<String>>,
/// }
/// ```
///
/// This generates:
/// - `AudioMetadataField` enum with variants: Instrument, Performer, MultiMic
/// - `AudioMetadataValue` enum with variants holding the respective types
/// - Complete `Metadata` trait implementation
/// - Extension trait `AudioMetadataGroupExt` with methods on `GroupBuilder<AudioMetadata>`:
///   - `.instrument<F: IntoField<AudioMetadata>>(field: F) -> Self`
///   - `.performer<F: IntoField<AudioMetadata>>(field: F) -> Self`
///   - `.multi_mic<F: IntoField<AudioMetadata>>(field: F) -> Self`
///
/// Supports attributes:
/// - `#[metadata(skip)]` - Exclude field from metadata
/// - `#[monarchy(variant)]` - Mark field as a variant (for grouping items that are identical except for this field)
///
/// Note: Tagged collections are now defined at the group level using `.tagged_collection(group)`
/// and work at the pattern matching level, not the metadata field level.
#[proc_macro_derive(Metadata, attributes(metadata, monarchy))]
pub fn derive_metadata(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    // Get the struct name
    let struct_name = &input.ident;

    // Generate field and value enum names
    let field_enum_name = Ident::new(&format!("{}Field", struct_name), Span::call_site());
    let value_enum_name = Ident::new(&format!("{}Value", struct_name), Span::call_site());

    // Extract fields from the struct
    let fields = match &input.data {
        Data::Struct(data_struct) => match &data_struct.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return syn::Error::new(
                    Span::call_site(),
                    "Metadata can only be derived for structs with named fields",
                )
                .to_compile_error()
                .into();
            }
        },
        _ => {
            return syn::Error::new(
                Span::call_site(),
                "Metadata can only be derived for structs",
            )
            .to_compile_error()
            .into();
        }
    };

    // Collect variant fields
    let mut variant_fields = Vec::new();

    // Collect field information
    let field_info: Vec<FieldInfo> = fields
        .iter()
        .filter_map(|field| {
            let field_name = field.ident.as_ref()?;
            let field_type = extract_option_type(&field.ty)?;

            // Check for #[monarchy(variant)] attribute
            let is_variant = field.attrs.iter().any(|attr| {
                if attr.path().is_ident("monarchy") {
                    attr.parse_args::<syn::Ident>()
                        .map(|i| i == "variant")
                        .unwrap_or(false)
                } else {
                    false
                }
            });

            // Check for #[metadata(skip)] attribute
            let skip = field.attrs.iter().any(|attr| {
                if attr.path().is_ident("metadata") {
                    attr.parse_args::<syn::Ident>()
                        .map(|i| i == "skip")
                        .unwrap_or(false)
                } else {
                    false
                }
            });

            if skip {
                return None;
            }

            let variant_name = Ident::new(
                &field_name.to_string().to_case(Case::Pascal),
                field_name.span(),
            );

            if is_variant {
                variant_fields.push(variant_name.clone());
            }

            Some(FieldInfo {
                field_name: field_name.clone(),
                variant_name,
                field_type: field_type.clone(),
            })
        })
        .collect();

    // Generate field enum variants
    let field_variants = field_info.iter().map(|info| {
        let variant = &info.variant_name;
        quote! { #variant }
    });

    // Generate value enum variants
    let value_variants = field_info.iter().map(|info| {
        let variant = &info.variant_name;
        let ty = &info.field_type;
        quote! { #variant(#ty) }
    });

    // Generate match arms for get()
    let get_arms = field_info.iter().map(|info| {
        let variant = &info.variant_name;
        let field = &info.field_name;
        quote! {
            #field_enum_name::#variant => {
                self.#field.clone().map(#value_enum_name::#variant)
            }
        }
    });

    // Generate match arms for set()
    let set_arms = field_info.iter().map(|info| {
        let variant = &info.variant_name;
        let field = &info.field_name;
        quote! {
            (#field_enum_name::#variant, #value_enum_name::#variant(v)) => {
                self.#field = Some(v);
            }
        }
    });

    // Generate fields() implementation
    let field_list = field_info.iter().map(|info| {
        let variant = &info.variant_name;
        quote! { #field_enum_name::#variant }
    });

    // Generate variant_fields() implementation
    let variant_field_list = variant_fields.iter().map(|variant| {
        quote! { #field_enum_name::#variant }
    });

    // Generate create_string_value match arms
    // Only for fields that are Option<String>, not Option<Vec<String>>
    let create_string_arms = field_info.iter().filter_map(|info| {
        let variant = &info.variant_name;
        let ty = &info.field_type;

        // Check if this is Vec<String> - if so, skip (use create_vec_string_value instead)
        if is_vec_string_type(ty) {
            return None;
        }

        Some(quote! {
            #field_enum_name::#variant => {
                Some(#value_enum_name::#variant(value))
            }
        })
    });

    // Generate create_vec_string_value match arms
    // Only for fields that are Option<Vec<String>>
    let create_vec_string_arms = field_info.iter().filter_map(|info| {
        let variant = &info.variant_name;
        let ty = &info.field_type;

        // Only include if this is Vec<String>
        if !is_vec_string_type(ty) {
            return None;
        }

        Some(quote! {
            #field_enum_name::#variant => {
                Some(#value_enum_name::#variant(values))
            }
        })
    });

    // Generate extension trait methods that call the generic metadata_field() method
    // We use an extension trait because we can't implement methods on GroupBuilder<M>
    // from outside the monarchy crate (orphan rule)
    let trait_name = Ident::new(&format!("{}GroupExt", struct_name), Span::call_site());

    let field_methods: Vec<_> = field_info
        .iter()
        .map(|info| {
            let field_name = &info.field_name;
            quote! {
                /// Add configuration for the #field_name metadata field
                ///
                /// This is a convenience method that calls `metadata_field()`.
                /// Accepts a Group that defines patterns, negative patterns,
                /// prefixes, nested groups, etc. for this specific metadata field.
                fn #field_name<F>(self, field_group: F) -> monarchy::GroupBuilder<#struct_name>
                where
                    F: monarchy::IntoField<#struct_name>;
            }
        })
        .collect();

    let impl_methods: Vec<_> = field_info
        .iter()
        .map(|info| {
            let field_name = &info.field_name;
            let variant_name = &info.variant_name;
            quote! {
                /// Add configuration for the #field_name metadata field
                fn #field_name<F>(self, field_group: F) -> monarchy::GroupBuilder<#struct_name>
                where
                    F: monarchy::IntoField<#struct_name>,
                {
                    self.metadata_field(#field_enum_name::#variant_name, field_group)
                }
            }
        })
        .collect();

    // Generate FieldName match arms
    let field_name_arms = field_info.iter().map(|info| {
        let variant = &info.variant_name;
        let variant_str = variant.to_string();
        quote! {
            #field_enum_name::#variant => #variant_str
        }
    });

    // Generate the output tokens
    let output = quote! {
        // Field enum
        #[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize, ::monarchy::facet::Facet)]
        #[repr(u8)]
        #[serde(rename_all = "snake_case")]
        pub enum #field_enum_name {
            #(#field_variants),*
        }

        // FieldName trait implementation
        impl monarchy::FieldName for #field_enum_name {
            fn name(&self) -> &'static str {
                match self {
                    #(#field_name_arms),*
                }
            }
        }

        // Value enum
        #[derive(Debug, Clone, ::monarchy::facet::Facet)]
        #[repr(C)]
        pub enum #value_enum_name {
            #(#value_variants),*
        }

        // Metadata trait implementation
        impl monarchy::Metadata for #struct_name {
            type Field = #field_enum_name;
            type Value = #value_enum_name;

            fn get(&self, field: &Self::Field) -> Option<Self::Value> {
                match field {
                    #(#get_arms),*
                }
            }

            fn set(&mut self, field: Self::Field, value: Self::Value) {
                match (field, value) {
                    #(#set_arms),*
                    _ => {}
                }
            }

            fn fields() -> Vec<Self::Field> {
                vec![
                    #(#field_list),*
                ]
            }

            fn variant_fields() -> Vec<Self::Field> {
                vec![
                    #(#variant_field_list),*
                ]
            }

            fn create_string_value(field: &Self::Field, value: String) -> Option<Self::Value> {
                match field {
                    #(#create_string_arms),*
                    _ => None,
                }
            }

            fn create_vec_string_value(field: &Self::Field, values: Vec<String>) -> Option<Self::Value> {
                match field {
                    #(#create_vec_string_arms),*
                    _ => None,
                }
            }
        }

        // Extension trait for GroupBuilder<#struct_name> (automatically included with Metadata derive)
        // We use a trait because we can't implement methods on GroupBuilder<M> from outside the monarchy crate
        /// Extension trait for GroupBuilder<#struct_name> with convenience methods for each metadata field
        pub trait #trait_name {
            #(#field_methods)*
        }

        impl #trait_name for monarchy::GroupBuilder<#struct_name> {
            #(#impl_methods)*
        }
    };

    output.into()
}

struct FieldInfo {
    field_name: Ident,
    variant_name: Ident,
    field_type: Type,
}

/// Derive macro for generating GroupBuilder extension methods for metadata fields
///
/// **Note:** This functionality is now automatically included when deriving `Metadata`.
/// You only need to derive `Metadata` to get both the trait implementation and
/// the GroupBuilder extension methods. This derive macro is kept for backwards
/// compatibility but is no longer necessary.
///
/// This macro generates an extension trait that adds methods to `GroupBuilder<M>`
/// for each metadata field, allowing you to call `.multi_mic()`, `.layers()`, etc.
///
/// # Example
///
/// ```ignore
/// use monarchy::Metadata;
///
/// #[derive(Metadata)]  // MetadataBuilder is now included automatically
/// struct AudioMetadata {
///     multi_mic: Option<Vec<String>>,
///     layers: Option<String>,
/// }
/// ```
///
/// This generates methods on `GroupBuilder<AudioMetadata>`:
/// - `multi_mic<F: IntoField<AudioMetadata>>(field: F) -> Self`
/// - `layers<F: IntoField<AudioMetadata>>(field: F) -> Self`
#[proc_macro_derive(MetadataBuilder, attributes(metadata, monarchy))]
pub fn derive_metadata_builder(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = &input.ident;
    let trait_name = Ident::new(&format!("{}GroupExt", struct_name), Span::call_site());

    let fields = match &input.data {
        Data::Struct(data_struct) => match &data_struct.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return syn::Error::new(
                    Span::call_site(),
                    "MetadataBuilder can only be derived for structs with named fields",
                )
                .to_compile_error()
                .into();
            }
        },
        _ => {
            return syn::Error::new(
                Span::call_site(),
                "MetadataBuilder can only be derived for structs",
            )
            .to_compile_error()
            .into();
        }
    };

    let field_methods: Vec<_> = fields
        .iter()
        .filter_map(|field| {
            let field_name = field.ident.as_ref()?;
            let _field_enum_name = Ident::new(&format!("{}Field", struct_name), Span::call_site());

            // Get the field enum variant name (PascalCase)
            let _variant_name = Ident::new(
                &field_name.to_string().to_case(Case::Pascal),
                field_name.span(),
            );

            // Check for #[metadata(skip)] attribute
            let skip = field.attrs.iter().any(|attr| {
                if attr.path().is_ident("metadata") {
                    attr.parse_args::<syn::Ident>()
                        .map(|i| i == "skip")
                        .unwrap_or(false)
                } else {
                    false
                }
            });

            if skip {
                return None;
            }

            Some(quote! {
                /// Add configuration for the #field_name metadata field
                ///
                /// This accepts a Group that defines patterns, negative patterns,
                /// prefixes, nested groups, etc. for this specific metadata field.
                fn #field_name<F>(self, field: F) -> monarchy::GroupBuilder<#struct_name>
                where
                    F: monarchy::IntoField<#struct_name>;
            })
        })
        .collect();

    // Generate implementations that work directly with GroupBuilder<M>
    let impl_methods: Vec<_> = fields
        .iter()
        .filter_map(|field| {
            let field_name = field.ident.as_ref()?;
            let field_enum_name = Ident::new(&format!("{}Field", struct_name), Span::call_site());
            let variant_name = Ident::new(
                &field_name.to_string().to_case(Case::Pascal),
                field_name.span(),
            );

            let skip = field.attrs.iter().any(|attr| {
                if attr.path().is_ident("metadata") {
                    attr.parse_args::<syn::Ident>()
                        .map(|i| i == "skip")
                        .unwrap_or(false)
                } else {
                    false
                }
            });

            if skip {
                return None;
            }

            Some(quote! {
                /// Add configuration for the #field_name metadata field
                fn #field_name<F>(mut self, field: F) -> monarchy::GroupBuilder<#struct_name>
                where
                    F: monarchy::IntoField<#struct_name>,
                {
                    let field_group = field.into_field();
                    self.group.metadata_fields.push(#field_enum_name::#variant_name);
                    self.group.patterns.extend(field_group.patterns);
                    self.group.negative_patterns.extend(field_group.negative_patterns);
                    if let Some(prefix) = field_group.prefix {
                        self.group.prefix = Some(prefix);
                    }
                    self.group.groups.extend(field_group.groups);
                    if field_group.priority > self.group.priority {
                        self.group.priority = field_group.priority;
                    }
                    self
                }
            })
        })
        .collect();

    let output = quote! {
        /// Extension trait for GroupBuilder<#struct_name> with methods for each metadata field
        pub trait #trait_name {
            #(#field_methods)*
        }

        impl #trait_name for monarchy::GroupBuilder<#struct_name> {
            #(#impl_methods)*
        }
    };

    output.into()
}

/// Extract the inner type from Option<T>
fn extract_option_type(ty: &Type) -> Option<&Type> {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            if segment.ident == "Option" {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(syn::GenericArgument::Type(inner_type)) = args.args.first() {
                        return Some(inner_type);
                    }
                }
            }
        }
    }
    None
}

/// Check if a type is Vec<String>
fn is_vec_string_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            if segment.ident == "Vec" {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(syn::GenericArgument::Type(Type::Path(inner_path))) =
                        args.args.first()
                    {
                        if let Some(inner_segment) = inner_path.path.segments.last() {
                            return inner_segment.ident == "String";
                        }
                    }
                }
            }
        }
    }
    false
}
