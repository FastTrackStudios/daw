//! Proc-macro crate for the `#[reaper_test]` attribute.
//!
//! Transforms:
//! ```ignore
//! #[reaper_test]
//! async fn test_something(ctx: &ReaperTestContext) -> Result<()> {
//!     let project = ctx.project();
//!     // ...
//!     Ok(())
//! }
//! ```
//!
//! Into:
//! ```ignore
//! #[test]
//! #[ignore = "reaper-test: run with `cargo xtask reaper-test`"]
//! fn test_something() -> eyre::Result<()> {
//!     reaper_test::run_reaper_test(
//!         "test_something",
//!         false,
//!         |ctx| Box::pin(async move {
//!             let project = ctx.project();
//!             // ...
//!             Ok(())
//!         }),
//!     )
//! }
//! ```

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn, Meta};

/// Attribute macro for REAPER integration tests.
///
/// Automatically adds `#[test]` and `#[ignore]`, wraps the test body
/// in `reaper_test::run_reaper_test()` which handles connection, project
/// tab assignment, and cleanup.
///
/// # Arguments
///
/// - `isolated` — optional, gives this test its own REAPER project tab.
///   Without `isolated`, the test joins a batch of tests sharing one tab.
///   Use `isolated` for tests that do destructive operations like
///   removing all tracks or needing a guaranteed-clean project.
///
/// # Examples
///
/// ```ignore
/// // Joins a batch — shares a project tab with other tests
/// #[reaper_test]
/// async fn test_add_fx(ctx: &ReaperTestContext) -> Result<()> {
///     let track = ctx.project().tracks().add("My Track", None).await?;
///     Ok(())
/// }
///
/// // Gets its own project tab
/// #[reaper_test(isolated)]
/// async fn test_destructive(ctx: &ReaperTestContext) -> Result<()> {
///     ctx.project().tracks().remove_all().await?;
///     Ok(())
/// }
///
/// // Load a template in the test body (works with batched or isolated)
/// #[reaper_test]
/// async fn test_with_template(ctx: &ReaperTestContext) -> Result<()> {
///     ctx.load_template("testing-stockjs-guitar-rig").await?;
///     let track = ctx.track_by_name("GUITAR Rig").await?;
///     Ok(())
/// }
/// ```
#[proc_macro_attribute]
pub fn reaper_test(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(item as ItemFn);

    let mut isolated = false;

    if !attr.is_empty() {
        let attr_str = attr.to_string();
        if attr_str.trim() == "isolated" {
            isolated = true;
        } else {
            let meta = parse_macro_input!(attr as Meta);
            match meta {
                Meta::Path(path) if path.is_ident("isolated") => {
                    isolated = true;
                }
                _ => {
                    return syn::Error::new_spanned(
                        proc_macro2::TokenStream::from(TokenStream::from(quote! { #meta })),
                        "expected `isolated` or no arguments",
                    )
                    .to_compile_error()
                    .into();
                }
            }
        }
    }

    let fn_name = &input_fn.sig.ident;
    let fn_block = &input_fn.block;
    let fn_vis = &input_fn.vis;
    let fn_attrs = &input_fn.attrs;

    // Validate: must be async
    if input_fn.sig.asyncness.is_none() {
        return syn::Error::new_spanned(
            &input_fn.sig.fn_token,
            "reaper_test functions must be async",
        )
        .to_compile_error()
        .into();
    }

    // Strip the ctx parameter — the closure will provide it.
    let ctx_ident = if input_fn.sig.inputs.len() == 1 {
        let arg = input_fn.sig.inputs.first().unwrap();
        match arg {
            syn::FnArg::Typed(pat_type) => match pat_type.pat.as_ref() {
                syn::Pat::Ident(pat_ident) => pat_ident.ident.clone(),
                _ => {
                    return syn::Error::new_spanned(
                        &pat_type.pat,
                        "expected a simple identifier for the context parameter",
                    )
                    .to_compile_error()
                    .into();
                }
            },
            _ => {
                return syn::Error::new_spanned(arg, "expected a typed parameter, not self")
                    .to_compile_error()
                    .into();
            }
        }
    } else if input_fn.sig.inputs.is_empty() {
        syn::Ident::new("_ctx", proc_macro2::Span::call_site())
    } else {
        return syn::Error::new_spanned(
            &input_fn.sig.inputs,
            "reaper_test functions must have zero or one parameter (ctx: &ReaperTestContext)",
        )
        .to_compile_error()
        .into();
    };

    let fn_name_str = fn_name.to_string();
    let output = quote! {
        #(#fn_attrs)*
        #[test]
        #[ignore = "reaper-test: run with `cargo xtask reaper-test`"]
        #fn_vis fn #fn_name() -> eyre::Result<()> {
            reaper_test::run_reaper_test(
                #fn_name_str,
                #isolated,
                |#ctx_ident| Box::pin(async move #fn_block),
            )
        }
    };

    output.into()
}
