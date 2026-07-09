//! Declarative macros for the actions system.
//!
//! Entry-point: [`define_actions!`].
//! It supports:
//! - Grouped action definitions (`prefix` + `title`)
//! - Grouped local action implementations (`implementation: supported(...)|unsupported(...)`)
//! - Legacy all-in-one dispatch generation (`for Type`)

/// Generate `dispatch_action()` for split library/binary pattern.
#[macro_export]
macro_rules! impl_action_dispatch {
    (
        for $impl_type:ty, use $mod_path:path {
            $(
                $const_name:ident => handler($self_:ident, $cx:ident) $handler:block
            )*
        }
    ) => {
        impl $impl_type {
            pub async fn dispatch_action(
                &self,
                cx: &::vox::session::Context,
                action_id: &$crate::ActionId,
            ) -> Option<$crate::ActionResult> {
                use $mod_path as __action_ids;
                let __id_str = action_id.as_str();
                $(
                    if __id_str == __action_ids::$const_name.as_str() {
                        let $self_ = self;
                        let $cx = cx;
                        let __result: $crate::ActionResult = { $handler };
                        return Some(__result);
                    }
                )*
                None
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __define_actions_local_impl_expr {
    (supported($handler:expr)) => {
        $crate::LocalActionImplementation::Supported(::std::sync::Arc::new($handler))
    };
    (unsupported($reason:expr)) => {
        $crate::LocalActionImplementation::Unsupported($reason)
    };
}

/// Unified actions macro.
#[macro_export]
macro_rules! define_actions {
    // Grouped definitions + local implementations (title omitted)
    (
        $(#[$mod_meta:meta])*
        $vis:vis $mod_name:ident {
            prefix: $prefix:literal,
            $(
                $const_name:ident = $id_suffix:literal {
                    name: $name:literal,
                    description: $desc:literal,
                    category: $category:ident
                    $(, group: $group:expr)?
                    $(, shortcut: $shortcut:literal)?
                    $(, when: $when:literal)?
                    , implementation: $impl_kind:ident ( $impl_value:expr )
                    $(,)?
                }
            )*
        }
    ) => {
        $crate::define_actions! {
            $(#[$mod_meta])*
            $vis $mod_name {
                prefix: $prefix,
                title: "",
                $(
                    $const_name = $id_suffix {
                        name: $name,
                        description: $desc,
                        category: $category
                        $(, group: $group)?
                        $(, shortcut: $shortcut)?
                        $(, when: $when)?
                        , implementation: $impl_kind($impl_value)
                    }
                )*
            }
        }
    };

    // Grouped definitions only + strict binder support (title omitted)
    (
        $(#[$mod_meta:meta])*
        $vis:vis $mod_name:ident {
            prefix: $prefix:literal,
            $(
                $const_name:ident = $id_suffix:literal {
                    name: $name:literal,
                    description: $desc:literal,
                    category: $category:ident
                    $(, group: $group:expr)?
                    $(, shortcut: $shortcut:literal)?
                    $(, when: $when:literal)?
                    $(,)?
                }
            )*
        }
    ) => {
        $crate::define_actions! {
            $(#[$mod_meta])*
            $vis $mod_name {
                prefix: $prefix,
                title: "",
                $(
                    $const_name = $id_suffix {
                        name: $name,
                        description: $desc,
                        category: $category
                        $(, group: $group)?
                        $(, shortcut: $shortcut)?
                        $(, when: $when)?
                    }
                )*
            }
        }
    };

    // Grouped definitions + local implementations
    (
        $(#[$mod_meta:meta])*
        $vis:vis $mod_name:ident {
            prefix: $prefix:literal,
            title: $title:expr,
            $(parent: $parent:expr,)?
            $(
                $const_name:ident = $id_suffix:literal {
                    name: $name:literal,
                    description: $desc:literal,
                    category: $category:ident
                    $(, group: $group:expr)?
                    $(, shortcut: $shortcut:literal)?
                    $(, when: $when:literal)?
                    , implementation: $impl_kind:ident ( $impl_value:expr )
                    $(,)?
                }
            )*
        }
    ) => {
        $(#[$mod_meta])*
        $vis mod $mod_name {
            use $crate::ids::StaticActionId;
            pub const TITLE: &str = $title;

            fn __menu_root() -> String {
                let mut root = String::from("FTS");
                $(
                    {
                        let parent = $parent;
                        let parent = parent.trim_matches('/');
                        if !parent.is_empty() {
                            root.push('/');
                            root.push_str(parent);
                        }
                    }
                )?
                if !TITLE.is_empty() {
                    root.push('/');
                    root.push_str(TITLE);
                }
                root
            }

            $(
                pub const $const_name: StaticActionId =
                    StaticActionId::new(concat!($prefix, ".", $id_suffix));
            )*

            #[allow(non_snake_case)]
            pub trait LocalActionBinder {
                $(fn $const_name(&self) -> $crate::LocalActionImplementation;)*
            }

            pub fn definitions() -> Vec<$crate::ActionDefinition> {
                vec![
                    $(
                        $crate::ActionDefinition::new(concat!($prefix, ".", $id_suffix), $name, $desc)
                            .with_category($crate::ActionCategory::$category)
                            .with_menu_path({
                                let mut path = __menu_root();
                                $(
                                    {
                                        let group = $group;
                                        let group = group.trim_matches('/');
                                        if !group.is_empty() {
                                            path.push('/');
                                            path.push_str(group);
                                        }
                                    }
                                )?
                                path
                            })
                            $(.with_shortcut($shortcut))?
                            $(.with_when($when))?
                            ,
                    )*
                ]
            }

            pub fn definitions_with_binder<B: LocalActionBinder>(
                binder: &B,
            ) -> Vec<$crate::LocalActionRegistration> {
                vec![
                    $(
                        $crate::LocalActionRegistration {
                            definition: $crate::ActionDefinition::new(
                                concat!($prefix, ".", $id_suffix), $name, $desc
                            )
                            .with_category($crate::ActionCategory::$category)
                            .with_menu_path({
                                let mut path = __menu_root();
                                $(
                                    {
                                        let group = $group;
                                        let group = group.trim_matches('/');
                                        if !group.is_empty() {
                                            path.push('/');
                                            path.push_str(group);
                                        }
                                    }
                                )?
                                path
                            })
                            $(.with_shortcut($shortcut))?
                            $(.with_when($when))?
                            ,
                            implementation: binder.$const_name(),
                        },
                    )*
                ]
            }

            pub fn definitions_with_handlers() -> Vec<$crate::LocalActionRegistration> {
                vec![
                    $(
                        $crate::LocalActionRegistration {
                            definition: $crate::ActionDefinition::new(
                                concat!($prefix, ".", $id_suffix), $name, $desc
                            )
                            .with_category($crate::ActionCategory::$category)
                            .with_menu_path({
                                let mut path = __menu_root();
                                $(
                                    {
                                        let group = $group;
                                        let group = group.trim_matches('/');
                                        if !group.is_empty() {
                                            path.push('/');
                                            path.push_str(group);
                                        }
                                    }
                                )?
                                path
                            })
                            $(.with_shortcut($shortcut))?
                            $(.with_when($when))?
                            ,
                            implementation: $crate::__define_actions_local_impl_expr!(
                                $impl_kind($impl_value)
                            ),
                        },
                    )*
                ]
            }
        }
    };

    // Grouped definitions only + strict binder support
    (
        $(#[$mod_meta:meta])*
        $vis:vis $mod_name:ident {
            prefix: $prefix:literal,
            title: $title:expr,
            $(parent: $parent:expr,)?
            $(
                $const_name:ident = $id_suffix:literal {
                    name: $name:literal,
                    description: $desc:literal,
                    category: $category:ident
                    $(, group: $group:expr)?
                    $(, shortcut: $shortcut:literal)?
                    $(, when: $when:literal)?
                    $(,)?
                }
            )*
        }
    ) => {
        $(#[$mod_meta])*
        $vis mod $mod_name {
            use $crate::ids::StaticActionId;
            pub const TITLE: &str = $title;

            fn __menu_root() -> String {
                let mut root = String::from("FTS");
                $(
                    {
                        let parent = $parent;
                        let parent = parent.trim_matches('/');
                        if !parent.is_empty() {
                            root.push('/');
                            root.push_str(parent);
                        }
                    }
                )?
                if !TITLE.is_empty() {
                    root.push('/');
                    root.push_str(TITLE);
                }
                root
            }

            $(
                pub const $const_name: StaticActionId =
                    StaticActionId::new(concat!($prefix, ".", $id_suffix));
            )*

            #[allow(non_snake_case)]
            pub trait LocalActionBinder {
                $(fn $const_name(&self) -> $crate::LocalActionImplementation;)*
            }

            pub fn definitions() -> Vec<$crate::ActionDefinition> {
                vec![
                    $(
                        $crate::ActionDefinition::new(concat!($prefix, ".", $id_suffix), $name, $desc)
                            .with_category($crate::ActionCategory::$category)
                            .with_menu_path({
                                let mut path = __menu_root();
                                $(
                                    {
                                        let group = $group;
                                        let group = group.trim_matches('/');
                                        if !group.is_empty() {
                                            path.push('/');
                                            path.push_str(group);
                                        }
                                    }
                                )?
                                path
                            })
                            $(.with_shortcut($shortcut))?
                            $(.with_when($when))?
                            ,
                    )*
                ]
            }

            pub fn definitions_with_binder<B: LocalActionBinder>(
                binder: &B,
            ) -> Vec<$crate::LocalActionRegistration> {
                vec![
                    $(
                        $crate::LocalActionRegistration {
                            definition: $crate::ActionDefinition::new(
                                concat!($prefix, ".", $id_suffix), $name, $desc
                            )
                            .with_category($crate::ActionCategory::$category)
                            .with_menu_path({
                                let mut path = __menu_root();
                                $(
                                    {
                                        let group = $group;
                                        let group = group.trim_matches('/');
                                        if !group.is_empty() {
                                            path.push('/');
                                            path.push_str(group);
                                        }
                                    }
                                )?
                                path
                            })
                            $(.with_shortcut($shortcut))?
                            $(.with_when($when))?
                            ,
                            implementation: binder.$const_name(),
                        },
                    )*
                ]
            }
        }
    };

    // Legacy all-in-one with dispatch method
    (
        $mod_name:ident for $impl_type:ty {
            $(
                $const_name:ident = $id_str:literal {
                    name: $name:literal,
                    description: $desc:literal,
                    category: $category:ident
                    $(, menu_path: $menu_path:literal )?
                    $(, shortcut: $shortcut:literal )?
                    $(, when: $when:literal )?
                    , handler($self_:ident, $cx:ident) $handler:block
                }
            )*
        }
    ) => {
        pub mod $mod_name {
            use $crate::ids::StaticActionId;
            $(pub const $const_name: StaticActionId = StaticActionId::new($id_str);)*
            pub fn definitions() -> Vec<$crate::ActionDefinition> {
                vec![
                    $(
                        $crate::ActionDefinition::new($id_str, $name, $desc)
                            .with_category($crate::ActionCategory::$category)
                            $(.with_menu_path($menu_path))?
                            $(.with_shortcut($shortcut))?
                            $(.with_when($when))?
                            ,
                    )*
                ]
            }
        }

        impl $impl_type {
            pub fn action_definitions() -> Vec<$crate::ActionDefinition> {
                $mod_name::definitions()
            }
            pub async fn dispatch_action(
                &self,
                cx: &::vox::session::Context,
                action_id: &$crate::ActionId,
            ) -> Option<$crate::ActionResult> {
                match action_id.as_str() {
                    $(
                        $id_str => {
                            let $self_ = self;
                            let $cx = cx;
                            let result: $crate::ActionResult = { $handler };
                            Some(result)
                        }
                    )*
                    _ => None,
                }
            }
        }
    };
}

/// Bind grouped definitions to runtime local implementations with fallback.
#[macro_export]
macro_rules! bind_grouped_local_actions {
    (
        module: $mod_path:path,
        unsupported: $unsupported_reason:expr,
        $(
            $const_name:ident => $impl_kind:ident ( $impl_value:expr )
        ),* $(,)?
    ) => {{
        use $mod_path as __action_mod;
        let mut __impls: ::std::collections::HashMap<String, $crate::LocalActionImplementation> =
            ::std::collections::HashMap::new();
        $(
            __impls.insert(
                __action_mod::$const_name.as_str().to_string(),
                $crate::__define_actions_local_impl_expr!($impl_kind($impl_value)),
            );
        )*
        __action_mod::definitions()
            .into_iter()
            .map(|definition| {
                let implementation = __impls.remove(definition.id.as_str()).unwrap_or(
                    $crate::LocalActionImplementation::Unsupported($unsupported_reason)
                );
                $crate::LocalActionRegistration { definition, implementation }
            })
            .collect::<Vec<$crate::LocalActionRegistration>>()
    }};
}
