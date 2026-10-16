//! What a project, track, item and take are called: one type each.
//!
//! A guid as a `String` can be any of them, and a clone of one is an
//! allocation. These are an `Arc<String>` underneath: a clone is a
//! reference count, a comparison is the string's, and the wire form is the
//! guid itself (`#[facet(transparent)]`), so an id crosses vox as the
//! string it always was. (`Arc<str>` would save a hop, but phon encodes a
//! smart pointer as its pointee and has no form for an unsized `str`.)
//!
//! ```
//! use daw_proto::id::ItemId;
//! use daw_proto::ItemRef;
//!
//! let item = ItemId::from("{A8FBEE09-88B0-9644-B9CE-AEDD4D33140F}");
//! let again = item.clone(); // a count, not a copy
//! assert_eq!(item, again);
//! let at: ItemRef = (&item).into();
//! assert!(matches!(at, ItemRef::Guid(g) if g == item.as_str()));
//! ```

use std::borrow::Borrow;
use std::fmt;
use std::sync::Arc;

use facet::Facet;

macro_rules! id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Facet)]
        #[facet(transparent)]
        pub struct $name(Arc<String>);

        impl $name {
            /// A new one, unlike any other: what a caller names a thing it is
            /// about to make, so an edit need not wait to learn it.
            #[must_use]
            pub fn new() -> Self {
                Self(Arc::new(uuid::Uuid::new_v4().to_string()))
            }

            /// The guid, as text.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            /// An empty id: no such thing.
            fn default() -> Self {
                Self(Arc::new(String::new()))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }

        impl From<&str> for $name {
            fn from(guid: &str) -> Self {
                Self(Arc::new(guid.to_owned()))
            }
        }

        impl From<String> for $name {
            fn from(guid: String) -> Self {
                Self(Arc::new(guid))
            }
        }

        impl From<&String> for $name {
            fn from(guid: &String) -> Self {
                Self(Arc::new(guid.clone()))
            }
        }

        impl Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.0.as_str() == other
            }
        }

        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.0.as_str() == *other
            }
        }

        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                *self.0 == *other
            }
        }
    };
}

id! {
    /// A project (a song): the guid the engine opened it as.
    ProjectId
}

id! {
    /// A track.
    TrackId
}

id! {
    /// An item on a track.
    ItemId
}

id! {
    /// A take of an item.
    TakeId
}

impl From<&ItemId> for crate::ItemRef {
    fn from(id: &ItemId) -> Self {
        crate::ItemRef::Guid(id.to_string())
    }
}

impl From<&TrackId> for crate::TrackRef {
    fn from(id: &TrackId) -> Self {
        crate::TrackRef::Guid(id.to_string())
    }
}

impl From<&ProjectId> for crate::ProjectContext {
    fn from(id: &ProjectId) -> Self {
        crate::ProjectContext::Project(id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{ItemId, TrackId};
    use facet::Facet;

    /// An item as it would cross vox: an id beside other fields.
    #[derive(Facet, Debug, PartialEq, Clone)]
    struct Moved {
        item: ItemId,
        track: TrackId,
        at: f64,
    }

    #[test]
    fn an_id_is_its_guid_on_the_wire() {
        let moved = Moved {
            item: ItemId::from("{A8FBEE09-88B0-9644-B9CE-AEDD4D33140F}"),
            track: TrackId::from("standalone-track-1"),
            at: 1.5,
        };
        let json = facet_json::to_string(&moved).expect("encodes");
        assert_eq!(
            json,
            r#"{"item":"{A8FBEE09-88B0-9644-B9CE-AEDD4D33140F}","track":"standalone-track-1","at":1.5}"#
        );
        let back: Moved = facet_json::from_str(&json).expect("decodes");
        assert_eq!(back, moved);
    }

    #[test]
    fn an_id_survives_phon_schema_exchange() {
        let moved = Moved {
            item: ItemId::new(),
            track: TrackId::new(),
            at: 0.25,
        };
        let bytes = vox_phon::to_vec(&moved).expect("encodes");
        let schema = vox_phon::schema_bytes::<Moved>().expect("schema");
        let bundle = vox_phon::parse_schema_bytes(&schema).expect("parses");
        let program = vox_phon::build_decode_program::<Moved>(&bundle).expect("program");
        let back: Moved = vox_phon::decode_owned_with_program(&program, &bytes).expect("decodes");
        assert_eq!(back, moved);
    }

    #[test]
    fn a_clone_shares_the_guid() {
        let id = ItemId::new();
        let again = id.clone();
        assert!(std::ptr::eq(id.as_str(), again.as_str()));
    }
}
