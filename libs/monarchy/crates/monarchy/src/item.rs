//! Item: Represents a parsed item with metadata
//!
//! This module contains the core [`Item`] struct which represents
//! a parsed input string with its associated metadata and matched groups.

use crate::{Group, Metadata, ToDisplayName};
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};
use std::rc::Rc;

// region:    --- Item

/// Represents a parsed item with metadata
///
/// An `Item` is the result of parsing an input string through the monarchy system.
/// It contains the original input, extracted metadata, and the chain of groups
/// that matched during parsing.
///
/// # Example
/// ```ignore
/// let parser = Parser::new(config);
/// let item = parser.parse("Kick In.wav")?;
/// assert_eq!(item.original, "Kick In.wav");
/// assert_eq!(item.matched_groups.last().unwrap().name, "Kick");
/// ```
#[derive(Clone, Debug)]
pub struct Item<M: Metadata> {
    /// Unique identifier for this item
    pub id: String,

    /// Original input string
    pub original: String,

    /// Parsed metadata
    pub metadata: M,

    /// Full hierarchy of groups that matched, from top-level to most specific.
    ///
    /// Stored as `Rc<Group<M>>` references for efficient sharing without cloning.
    /// Example: `[Drums Group, Drum_Kit Group, Kick Group]`
    /// The last element is the most specific group that matched.
    ///
    /// Note: For serialization, this is serialized as group names (`Vec<String>`).
    pub matched_groups: Vec<Rc<Group<M>>>,
}

impl<M: Metadata> Item<M> {
    /// Derive display name from matched groups and metadata
    ///
    /// This method extracts prefixes and group names from matched_groups,
    /// then delegates to the metadata's `to_display_name()` implementation.
    ///
    /// Returns the original name if metadata doesn't implement ToDisplayName
    /// or if to_display_name returns an empty string.
    ///
    /// # Example
    /// ```ignore
    /// let item = parser.parse("EdCrunch1.wav")?;
    /// let display = item.derive_display_name();
    /// // Returns something like "GTR E Ed Crunch 1"
    /// ```
    pub fn derive_display_name(&self) -> String
    where
        M: ToDisplayName,
    {
        let prefixes: Vec<String> = self
            .matched_groups
            .iter()
            .filter_map(|g| g.prefix.clone())
            .collect();

        let group_names: Vec<String> = self.matched_groups.iter().map(|g| g.name.clone()).collect();

        let display_name = self.metadata.to_display_name(&prefixes, &group_names);

        // Fall back to original if empty
        if display_name.is_empty() {
            self.original.clone()
        } else {
            display_name
        }
    }
}

// endregion: --- Item

// region:    --- Serialization

impl<M: Metadata> Serialize for Item<M>
where
    M: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Item", 4)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("original", &self.original)?;
        state.serialize_field("metadata", &self.metadata)?;
        let group_names: Vec<String> = self.matched_groups.iter().map(|g| g.name.clone()).collect();
        state.serialize_field("matched_groups", &group_names)?;
        state.end()
    }
}

impl<'de, M: Metadata> Deserialize<'de> for Item<M>
where
    M: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        struct ItemVisitor<M: Metadata>(std::marker::PhantomData<M>);

        impl<'de, M: Metadata> Visitor<'de> for ItemVisitor<M>
        where
            M: Deserialize<'de>,
        {
            type Value = Item<M>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct Item")
            }

            fn visit_map<V>(self, mut map: V) -> std::result::Result<Item<M>, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut id = None;
                let mut original = None;
                let mut metadata = None;
                let mut matched_group_names: Option<Vec<String>> = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        "id" => {
                            if id.is_some() {
                                return Err(de::Error::duplicate_field("id"));
                            }
                            id = Some(map.next_value()?);
                        }
                        "original" => {
                            if original.is_some() {
                                return Err(de::Error::duplicate_field("original"));
                            }
                            original = Some(map.next_value()?);
                        }
                        "metadata" => {
                            if metadata.is_some() {
                                return Err(de::Error::duplicate_field("metadata"));
                            }
                            metadata = Some(map.next_value()?);
                        }
                        "matched_groups" => {
                            if matched_group_names.is_some() {
                                return Err(de::Error::duplicate_field("matched_groups"));
                            }
                            matched_group_names = Some(map.next_value()?);
                        }
                        _ => {
                            let _ = map.next_value::<de::IgnoredAny>()?;
                        }
                    }
                }

                let id = id.ok_or_else(|| de::Error::missing_field("id"))?;
                let original = original.ok_or_else(|| de::Error::missing_field("original"))?;
                let metadata = metadata.ok_or_else(|| de::Error::missing_field("metadata"))?;
                let matched_group_names = matched_group_names.unwrap_or_default();

                // When deserializing, we can't reconstruct the full Group instances
                // So we create empty groups with just the names wrapped in Rc
                // This is a limitation - ideally we'd need the config to reconstruct properly
                let matched_groups = matched_group_names
                    .into_iter()
                    .map(|name| Rc::new(Group::builder(name).build()))
                    .collect();

                Ok(Item {
                    id,
                    original,
                    metadata,
                    matched_groups,
                })
            }
        }

        deserializer.deserialize_struct(
            "Item",
            &["id", "original", "metadata", "matched_groups"],
            ItemVisitor(std::marker::PhantomData),
        )
    }
}

// endregion: --- Serialization
