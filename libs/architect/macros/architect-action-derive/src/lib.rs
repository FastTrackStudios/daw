//! `#[architect::actions]` — named-command traits.
//!
//! Verb-shaped RPC services (`#[architect::rpc]`) model data operations:
//! `Tracks::set_muted(guid, bool)`. Actions are a different shape: a
//! user- or menu-triggered command with no meaningful return value, that
//! needs a stable id, a human description, and a place in a menu/CLI tree
//! (category + group). REAPER's named-command registry is the motivating
//! backend, but this crate has no REAPER awareness at all — that's the
//! whole point of the [`architect::action::ActionBackend`] seam.
//!
//! ```ignore
//! #[architect::actions(namespace = "SESSION")]
//! pub trait SetlistActions {
//!     #[action(description = "Scan open projects and rebuild the setlist", category = "Setlist", group = "Build")]
//!     fn build_setlist(&self);
//!
//!     #[action(description = "Stamp demo markers/regions into the current project", category = "Setlist", group = "Demo")]
//!     fn load_demo_setlist(&self);
//! }
//! ```
//!
//! Emits, alongside the trait unchanged (attributes stripped):
//!
//! - one `pub const <METHOD_SCREAMING>: ActionMeta` per `#[action(...)]` method
//! - `pub struct <Trait>Actions;` with `<Trait>Actions::all() -> &'static [ActionMeta]`
//! - `pub fn register_<trait_snake>_actions(backend, imp: Arc<dyn Trait + Send + Sync>)`
//!   — wires every action's handler through the given [`architect::action::ActionBackend`]
//!
//! Deliberately narrower than `#[architect::rpc]` for v1: action methods
//! must be `fn name(&self)` — no other params, no return value. REAPER
//! named commands take no arguments; if a richer shape is needed later
//! (e.g. toggle actions that report state), extend then.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Attribute, ItemTrait, TraitItem, parse_macro_input};

#[proc_macro_attribute]
pub fn actions(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as ActionsArgs);
    let trait_item = parse_macro_input!(input as ItemTrait);
    match expand(trait_item, args) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

struct ActionsArgs {
    namespace: Option<String>,
}

impl syn::parse::Parse for ActionsArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut namespace = None;
        let metas =
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated(input)?;
        for meta in metas {
            if meta.path().is_ident("namespace") {
                namespace = Some(lit_str_value(&meta)?);
            } else {
                return Err(syn::Error::new_spanned(
                    &meta,
                    "unknown #[architect::actions(...)] argument (expected `namespace`)",
                ));
            }
        }
        Ok(ActionsArgs { namespace })
    }
}

/// One parsed `#[action(...)]` method attribute.
struct ActionAttr {
    description: String,
    category: String,
    group: String,
    toggleable: bool,
    display_name: Option<String>,
}

fn lit_str_value(meta: &syn::Meta) -> syn::Result<String> {
    match meta {
        syn::Meta::NameValue(nv) => match &nv.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) => Ok(s.value()),
            other => Err(syn::Error::new_spanned(other, "expected a string literal")),
        },
        other => Err(syn::Error::new_spanned(other, "expected `key = \"value\"`")),
    }
}

fn parse_action_attr(attr: &Attribute) -> syn::Result<ActionAttr> {
    let mut description = None;
    let mut category = None;
    let mut group = None;
    let mut toggleable = false;
    let mut display_name = None;

    // Name-value pairs (`description = "..."`) mixed with a bare flag
    // (`toggleable`) — parse the attribute's meta list directly rather
    // than `parse_nested_meta`, which is awkward for that mix.
    let metas: syn::punctuated::Punctuated<syn::Meta, syn::Token![,]> =
        attr.parse_args_with(syn::punctuated::Punctuated::parse_terminated)?;

    for meta in &metas {
        if meta.path().is_ident("description") {
            description = Some(lit_str_value(meta)?);
        } else if meta.path().is_ident("category") {
            category = Some(lit_str_value(meta)?);
        } else if meta.path().is_ident("group") {
            group = Some(lit_str_value(meta)?);
        } else if meta.path().is_ident("display_name") {
            display_name = Some(lit_str_value(meta)?);
        } else if meta.path().is_ident("toggleable") {
            toggleable = match meta {
                syn::Meta::Path(_) => true,
                syn::Meta::NameValue(nv) => match &nv.value {
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Bool(b),
                        ..
                    }) => b.value,
                    other => {
                        return Err(syn::Error::new_spanned(
                            other,
                            "expected `toggleable` or `toggleable = true/false`",
                        ));
                    }
                },
                syn::Meta::List(l) => {
                    return Err(syn::Error::new_spanned(l, "unexpected `toggleable(...)`"));
                }
            };
        } else {
            return Err(syn::Error::new_spanned(
                meta,
                "unknown #[action(...)] argument (expected description/category/group/display_name/toggleable)",
            ));
        }
    }

    Ok(ActionAttr {
        description: description.ok_or_else(|| {
            syn::Error::new_spanned(attr, "#[action(...)] requires `description = \"...\"`")
        })?,
        category: category.unwrap_or_default(),
        group: group.unwrap_or_default(),
        toggleable,
        display_name,
    })
}

fn expand(mut trait_item: ItemTrait, args: ActionsArgs) -> syn::Result<TokenStream2> {
    let trait_name = &trait_item.ident;
    let trait_name_str = trait_name.to_string();
    let vis = &trait_item.vis;

    let namespace = args
        .namespace
        .unwrap_or_else(|| to_screaming_snake_case(strip_actions_suffix(&trait_name_str)));

    let mut const_defs = Vec::new();
    // (const_ident, cfg_attrs) — the cfg attrs are re-emitted everywhere
    // this const is referenced (the `all()` push, the register call) so
    // a `#[cfg]`-gated-off method disappears consistently everywhere,
    // instead of leaving a dangling reference to a method rustc already
    // stripped from the trait itself.
    let mut const_idents = Vec::new();
    let mut register_calls = Vec::new();

    for item in &mut trait_item.items {
        let TraitItem::Fn(method) = item else {
            continue;
        };

        let action_attr_idx = method
            .attrs
            .iter()
            .position(|a| a.path().is_ident("action"));
        let Some(idx) = action_attr_idx else {
            continue;
        };
        let attr = method.attrs.remove(idx);
        let parsed = parse_action_attr(&attr)?;

        // Collect (don't remove) any #[cfg(...)] on this method — it
        // must stay on the method itself so the trait's own definition
        // still strips it normally, and gets cloned onto every other
        // emission site below so they strip in lockstep.
        let cfg_attrs: Vec<&Attribute> = method
            .attrs
            .iter()
            .filter(|a| a.path().is_ident("cfg"))
            .collect();

        // v1 constraint: `fn name(&self)` only — no other params, no return
        // value. See module doc for why.
        let mut inputs = method.sig.inputs.iter();
        match inputs.next() {
            Some(syn::FnArg::Receiver(r)) if r.reference.is_some() && r.mutability.is_none() => {}
            _ => {
                return Err(syn::Error::new_spanned(
                    &method.sig,
                    "#[action] methods must take `&self` as their only parameter",
                ));
            }
        }
        if inputs.next().is_some() {
            return Err(syn::Error::new_spanned(
                &method.sig,
                "#[action] methods must take no parameters besides `&self` (v1 constraint — REAPER named commands take no arguments)",
            ));
        }
        if !matches!(method.sig.output, syn::ReturnType::Default) {
            return Err(syn::Error::new_spanned(
                &method.sig,
                "#[action] methods must not return a value (v1 constraint)",
            ));
        }

        let method_name = &method.sig.ident;
        let method_name_str = method_name.to_string();
        let const_ident = format_ident!("{}", to_screaming_snake_case(&method_name_str));
        let id = format!("{namespace}_{}", to_screaming_snake_case(&method_name_str));
        let display_name = parsed
            .display_name
            .unwrap_or_else(|| to_title_case(&method_name_str));
        let description = &parsed.description;
        let category = &parsed.category;
        let group = &parsed.group;
        let toggleable = parsed.toggleable;

        const_defs.push(quote! {
            #(#cfg_attrs)*
            #vis const #const_ident: ::architect::action::ActionMeta = ::architect::action::ActionMeta {
                id: #id,
                trait_name: #trait_name_str,
                method_name: #method_name_str,
                display_name: #display_name,
                description: #description,
                category: #category,
                group: #group,
                toggleable: #toggleable,
            };
        });
        const_idents.push((
            const_ident.clone(),
            cfg_attrs.iter().map(|a| quote! { #a }).collect::<Vec<_>>(),
        ));

        register_calls.push(quote! {
            #(#cfg_attrs)*
            {
                let imp = ::std::clone::Clone::clone(&imp);
                backend.register(&#const_ident, ::std::sync::Arc::new(move || {
                    #trait_name::#method_name(&*imp);
                }));
            }
        });
    }

    if const_idents.is_empty() {
        return Err(syn::Error::new_spanned(
            &trait_item,
            "#[architect::actions] trait has no #[action(...)]-decorated methods",
        ));
    }

    let actions_marker = format_ident!("{}Actions", trait_name);
    let register_fn = format_ident!("register_{}_actions", to_snake_case(&trait_name_str));

    // `all()` can't be a plain `&[A, B, C]` static array literal once any
    // element is `#[cfg]`-gated — array literals don't support per-element
    // cfg on stable Rust, and a gated-off ident wouldn't exist to name.
    // Building the list via cfg-gated `push` calls into a lazily-built,
    // process-lifetime `Vec` sidesteps that: each push vanishes in lockstep
    // with its const/method when the cfg is off, same as any other cfg'd
    // statement.
    let all_pushes = const_idents.iter().map(|(ident, cfg_attrs)| {
        quote! {
            #(#cfg_attrs)*
            v.push(#ident);
        }
    });

    Ok(quote! {
        #trait_item

        #(#const_defs)*

        /// All [`::architect::action::ActionMeta`] declared on
        #[doc = concat!("[`", stringify!(#trait_name), "`].")]
        #vis struct #actions_marker;

        impl #actions_marker {
            #vis fn all() -> &'static [::architect::action::ActionMeta] {
                static ALL: ::std::sync::OnceLock<::std::vec::Vec<::architect::action::ActionMeta>> =
                    ::std::sync::OnceLock::new();
                ALL.get_or_init(|| {
                    let mut v = ::std::vec::Vec::new();
                    #(#all_pushes)*
                    v
                })
            }
        }

        /// Registers every action on
        #[doc = concat!("[`", stringify!(#trait_name), "`]")]
        /// with `backend`, dispatching each through `imp`.
        #vis fn #register_fn<B, T>(backend: &B, imp: ::std::sync::Arc<T>)
        where
            B: ::architect::action::ActionBackend + ?Sized,
            T: #trait_name + Send + Sync + 'static,
        {
            #(#register_calls)*
        }
    })
}

fn strip_actions_suffix(name: &str) -> &str {
    name.strip_suffix("Actions").unwrap_or(name)
}

/// `BuildSetlist` / `build_setlist` -> `BUILD_SETLIST`
fn to_screaming_snake_case(input: &str) -> String {
    to_snake_case(input).to_uppercase()
}

/// `BuildSetlist` -> `build_setlist`. Idempotent on already-snake input.
fn to_snake_case(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 4);
    let mut prev_lower_or_digit = false;
    for ch in input.chars() {
        if ch == '_' {
            out.push('_');
            prev_lower_or_digit = false;
            continue;
        }
        if ch.is_uppercase() {
            if prev_lower_or_digit {
                out.push('_');
            }
            out.extend(ch.to_lowercase());
            prev_lower_or_digit = false;
        } else {
            out.push(ch);
            prev_lower_or_digit = ch.is_alphanumeric();
        }
    }
    out
}

/// `build_setlist` -> `Build Setlist`
fn to_title_case(input: &str) -> String {
    to_snake_case(input)
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
