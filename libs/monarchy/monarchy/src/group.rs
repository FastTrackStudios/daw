use crate::field_value::FieldValueDescriptor;
use crate::{IntoField, Metadata};
use facet::Facet;
use serde::{Deserialize, Serialize};

/// Strategy for how items should be grouped when a metadata field is present
#[repr(u8)]
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Facet)]
pub enum FieldGroupingStrategy {
    /// Default behavior: items with the field value become children grouped by value
    /// Items without the field stay at the current level
    Default,

    /// MainOnContainer: items WITHOUT the field go on the folder track (container),
    /// items WITH the field become children grouped by field value
    ///
    /// Example: For MultiMic field with MainOnContainer:
    /// - "Guitar Clean" (no MultiMic) → goes on "Clean" folder track
    /// - "Guitar Clean Amp" (MultiMic: "Amp") → becomes child under "Clean"
    /// - "Guitar Clean DI" (MultiMic: "DI") → becomes child under "Clean"
    MainOnContainer,
}

/// Helper trait to convert single items or collections into a Vec
///
/// This allows builder methods to accept both single items and collections:
/// - Single item: `"pattern"` or `field` or `group`
/// - Collection: `vec!["pattern1", "pattern2"]` or `[field1, field2]` or `[group1, group2]`
pub trait IntoVec<T> {
    fn into_vec(self) -> Vec<T>;
}

// Implementation for single item
impl<T> IntoVec<T> for T {
    fn into_vec(self) -> Vec<T> {
        vec![self]
    }
}

// Implementation for Vec<T>
impl<T> IntoVec<T> for Vec<T> {
    fn into_vec(self) -> Vec<T> {
        self
    }
}

// Implementation for arrays [T; N]
impl<T, const N: usize> IntoVec<T> for [T; N] {
    fn into_vec(self) -> Vec<T> {
        self.into_iter().collect()
    }
}

// Implementation for slices &[T]
impl<T: Clone> IntoVec<T> for &[T] {
    fn into_vec(self) -> Vec<T> {
        self.to_vec()
    }
}

// Special implementation for patterns: convert &str and collections of &str to Vec<String>
impl IntoVec<String> for &str {
    fn into_vec(self) -> Vec<String> {
        vec![self.to_string()]
    }
}

impl IntoVec<String> for Vec<&str> {
    fn into_vec(self) -> Vec<String> {
        self.into_iter().map(|s| s.to_string()).collect()
    }
}

impl<const N: usize> IntoVec<String> for [&str; N] {
    fn into_vec(self) -> Vec<String> {
        self.into_iter().map(|s| s.to_string()).collect()
    }
}

impl IntoVec<String> for &[&str] {
    fn into_vec(self) -> Vec<String> {
        self.iter().map(|s| (*s).to_string()).collect()
    }
}

/// Defines a group pattern for organizing items
#[derive(Clone, Debug, Serialize, Deserialize, Facet)]
pub struct Group<M: Metadata> {
    /// Name of this group (used for display)
    pub name: String,

    /// Optional unique identifier for this group (used for targeting in operations)
    /// Examples: "kick_drum_kit", "snare_electronic", "bgvs", "lead_vocals"
    /// If not set, the group can still be found by name, but id provides unambiguous targeting
    pub id: Option<String>,

    /// Optional prefix to apply to this group and its children
    pub prefix: Option<String>,

    /// Whether this group should inherit prefixes from parent groups
    pub inherit_prefix: bool,

    /// Specific prefixes to block from being inherited (e.g., ["D"] to avoid "D Drum Kit")
    pub blocked_prefixes: Vec<String>,

    /// Patterns to match (any of these will match)
    pub patterns: Vec<String>,

    /// Patterns to exclude (none of these should match)
    pub negative_patterns: Vec<String>,

    /// Which metadata fields this group cares about
    pub metadata_fields: Vec<M::Field>,

    /// Nested groups (like folders)
    pub groups: Vec<Group<M>>,

    /// Priority for matching (higher = checked first)
    pub priority: i32,

    /// Optional tagged collection group - items matching this group's patterns
    /// will be moved to a subfolder if there are also non-matching items
    /// Tagged collections work at the pattern matching level, not metadata field level
    /// Example: If tagged collection has patterns ["In", "Out", "Trig"] and we have
    /// items "In", "Out", "Trig", "Ambient", then "In", "Out", "Trig" go into
    /// a subfolder named after the tagged collection, and "Ambient" stays at parent level
    pub tagged_collection: Option<Box<Group<M>>>,

    /// Optional variant field - items with different values for this field
    /// will be kept together in the same structure but grouped by variant value
    /// for client-side handling (e.g., different lanes on the same track)
    pub variants: Option<M::Field>,

    /// If true, this group's children are promoted to the parent level in the output
    /// The group still exists for matching/organization but doesn't create nesting
    pub transparent: bool,

    /// If true, this group is used only for metadata extraction and does not create
    /// a structure node in the hierarchy. It will always match (or match with very
    /// low priority) to extract metadata fields like Section, MultiMic, etc.
    pub metadata_only: bool,

    /// If true, this group can only match if its parent group also matches.
    /// This is useful for groups like "Electronic Snare" that should only match
    /// when both "Electronic Kit" and "Snare" patterns are present.
    /// Default is false, allowing groups to match independently.
    pub requires_parent_match: bool,

    /// Field value descriptors for metadata fields
    ///
    /// This allows each metadata field value (e.g., "Out", "In", "Hi Hat", "Ride")
    /// to have its own patterns and negative patterns, making them act more like separate groups.
    ///
    /// The key is the field name (e.g., "MultiMic"), and the value is a list of descriptors
    /// for each possible value of that field.
    ///
    /// Example (conceptual structure):
    /// ```text
    /// field_value_descriptors: {
    ///     "MultiMic": [
    ///         FieldValueDescriptor { value: "Out", patterns: ["out"], negative_patterns: [] },
    ///         FieldValueDescriptor { value: "In", patterns: ["in"], negative_patterns: [] },
    ///     ]
    /// }
    /// ```
    pub field_value_descriptors: std::collections::HashMap<String, Vec<FieldValueDescriptor>>,

    /// Field grouping strategies for metadata fields
    ///
    /// This allows different metadata fields to use different grouping behaviors.
    /// The key is the field name (e.g., "MultiMic"), and the value is the strategy to use.
    ///
    /// If no strategy is specified for a field, `FieldGroupingStrategy::Default` is used.
    ///
    /// Example (conceptual structure):
    /// ```text
    /// field_grouping_strategies: {
    ///     "MultiMic": FieldGroupingStrategy::MainOnContainer,
    /// }
    /// ```
    pub field_grouping_strategies: std::collections::HashMap<String, FieldGroupingStrategy>,

    /// Default values for metadata fields when items don't have the field
    ///
    /// When grouping by a metadata field, items without that field will be treated
    /// as if they have the field with the default value. This allows items without
    /// a field to be grouped alongside items that do have the field.
    ///
    /// Example: For Layers field with default "Main":
    /// - "Guitar Clean" (no layer) → treated as "Guitar Clean" with layer "Main"
    /// - "Guitar Clean DBL" (layer: "DBL") → grouped alongside "Main" items
    ///
    /// The key is the field name (e.g., "Layers"), and the value is the default value (e.g., "Main").
    pub field_default_values: std::collections::HashMap<String, String>,
}

impl<M: Metadata> Group<M> {
    /// Start building a new group - this is the only way to create a Group
    pub fn builder(name: impl Into<String>) -> GroupBuilder<M> {
        GroupBuilder::new(name)
    }

    /// Check if a string matches this group's patterns
    ///
    /// If no patterns are specified, this returns `true` (matches everything).
    /// This allows container groups to match when any child matches.
    pub fn matches(&self, text: &str) -> bool {
        // Check negative patterns first
        for pattern in &self.negative_patterns {
            if crate::utils::contains_word(text, pattern) {
                return false;
            }
        }

        // Check positive patterns
        if self.patterns.is_empty() {
            // If no patterns specified, match by default
            return true;
        }

        for pattern in &self.patterns {
            if crate::utils::contains_word(text, pattern) {
                return true;
            }
        }

        false
    }

    /// Check if this group has any patterns defined
    pub fn has_patterns(&self) -> bool {
        !self.patterns.is_empty()
    }

    /// Check if text contains pattern as a whole word (not just as a substring)
    ///
    /// Delegates to [`crate::utils::contains_word`].
    #[deprecated(since = "0.2.0", note = "Use crate::utils::contains_word instead")]
    pub fn contains_word(text: &str, pattern: &str) -> bool {
        crate::utils::contains_word(text, pattern)
    }
}

impl<M: Metadata> GroupBuilder<M> {
    /// Create a new GroupBuilder with the given name
    pub fn new(name: impl Into<String>) -> Self {
        GroupBuilder {
            group: Group {
                name: name.into(),
                id: None,
                prefix: None,
                inherit_prefix: true,
                blocked_prefixes: Vec::new(),
                patterns: Vec::new(),
                negative_patterns: Vec::new(),
                metadata_fields: Vec::new(),
                groups: Vec::new(),
                priority: 0,
                tagged_collection: None,
                variants: None,
                transparent: false,
                metadata_only: false,
                requires_parent_match: false,
                field_value_descriptors: std::collections::HashMap::new(),
                field_grouping_strategies: std::collections::HashMap::new(),
                field_default_values: std::collections::HashMap::new(),
            },
        }
    }
}

/// Builder for creating Groups with a fluent API
#[derive(Clone, Debug, Facet)]
pub struct GroupBuilder<M: Metadata> {
    /// The group being built (public for extension traits)
    pub group: Group<M>,
}

impl<M: Metadata> GroupBuilder<M> {
    /// Add patterns to match (accepts single pattern or collection)
    pub fn patterns<P>(mut self, patterns: P) -> Self
    where
        P: IntoVec<String>,
    {
        self.group.patterns.extend(patterns.into_vec());
        self
    }

    /// Add exclusion patterns (accepts single pattern or collection)
    pub fn exclude<P>(mut self, patterns: P) -> Self
    where
        P: IntoVec<String>,
    {
        self.group.negative_patterns.extend(patterns.into_vec());
        self
    }

    /// Add metadata fields this group cares about (accepts single field or collection)
    pub fn field<F>(mut self, fields: F) -> Self
    where
        F: IntoVec<M::Field>,
    {
        self.group.metadata_fields.extend(fields.into_vec());
        self
    }

    /// Add configuration for a specific metadata field
    ///
    /// This is a generic method that works with any metadata field.
    /// The derive macro generates convenience methods (like `.multi_mic()`) that call this.
    ///
    /// # Example
    ///
    /// ```ignore
    /// Group::builder("Kick")
    ///     .metadata_field(ItemMetadataField::MultiMic, multi_mic_group)
    ///     .build()
    /// ```
    pub fn metadata_field<F>(mut self, field: M::Field, field_group: F) -> Self
    where
        F: IntoField<M>,
    {
        let field_group = field_group.into_field();
        self.group.metadata_fields.push(field);
        // Store the field_group itself as a nested group so we can find it later
        // This allows the parser to extract values from the field's patterns
        self.group.groups.push(field_group);
        self
    }

    /// Add field value descriptors for a metadata field
    ///
    /// This allows each metadata field value (e.g., "Out", "In", "Hi Hat", "Ride")
    /// to have its own patterns and negative patterns.
    ///
    /// # Example
    ///
    /// ```ignore
    /// Group::builder("Kick")
    ///     .field_value_descriptors(
    ///         ItemMetadataField::MultiMic,
    ///         vec![
    ///             FieldValueDescriptor::builder("Out").patterns(["out"]).build(),
    ///             FieldValueDescriptor::builder("In").patterns(["in"]).build(),
    ///         ]
    ///     )
    ///     .build()
    /// ```
    pub fn field_value_descriptors(
        mut self,
        field: M::Field,
        descriptors: Vec<FieldValueDescriptor>,
    ) -> Self {
        let field_name = format!("{:?}", field);
        self.group.metadata_fields.push(field);
        self.group
            .field_value_descriptors
            .insert(field_name, descriptors);
        self
    }

    /// Set a unique identifier for this group
    ///
    /// The ID is used for unambiguous targeting in operations like `move_child_to_group`.
    /// Convention: use snake_case with parent context, e.g., "kick_drum_kit", "snare_electronic"
    ///
    /// # Example
    ///
    /// ```ignore
    /// Group::builder("Kick")
    ///     .id("kick_drum_kit")
    ///     .patterns(["kick", "kik", "bd"])
    ///     .build()
    /// ```
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.group.id = Some(id.into());
        self
    }

    /// Set a prefix for this group
    pub fn prefix(mut self, prefix: impl Into<String>) -> Self {
        self.group.prefix = Some(prefix.into());
        self
    }

    /// Disable prefix inheritance for this group
    pub fn no_inherit_prefix(mut self) -> Self {
        self.group.inherit_prefix = false;
        self
    }

    /// Block prefixes from being inherited (accepts single prefix or collection)
    pub fn block_prefix<P>(mut self, prefixes: P) -> Self
    where
        P: IntoVec<String>,
    {
        self.group.blocked_prefixes.extend(prefixes.into_vec());
        self
    }

    /// Add nested groups (accepts single group or collection).
    ///
    /// Accepts anything that can be converted to `Group<M>`:
    /// - Single group: `Group<M>` or anything implementing `Into<Group<M>>`
    /// - Collections: `Vec<G>` or `[G; N]` where `G: Into<Group<M>>`
    pub fn group<G, I>(mut self, groups: G) -> Self
    where
        G: IntoVec<I>,
        I: Into<Group<M>>,
    {
        self.group
            .groups
            .extend(groups.into_vec().into_iter().map(Into::into));
        self
    }

    /// Set the priority for this group
    pub fn priority(mut self, priority: i32) -> Self {
        self.group.priority = priority;
        self
    }

    /// Set a tagged collection group for this group
    /// Items matching the tagged collection's patterns will be moved to a subfolder
    /// if there are also non-matching items. If all items match, they stay at current level.
    /// Tagged collections work at the pattern matching level, allowing cross-field collections
    /// (e.g., "In" from multi_mic and "DBL" from layers can be in the same collection)
    pub fn tagged_collection(mut self, collection_group: Group<M>) -> Self {
        self.group.tagged_collection = Some(Box::new(collection_group));
        self
    }

    /// Set the variant field for this group
    /// Items with different values for this field will be kept together in the same structure
    /// but grouped by variant value for client-side handling (e.g., different lanes on the same track)
    pub fn variants(mut self, field: M::Field) -> Self {
        self.group.variants = Some(field);
        self
    }

    /// Automatically use variant fields from metadata if they're marked with #[monarchy(variant)]
    pub fn use_auto_variants(mut self) -> Self {
        let variant_fields = M::variant_fields();
        if !variant_fields.is_empty() {
            // Use the first variant field if multiple are defined
            self.group.variants = Some(variant_fields[0].clone());
        }
        self
    }

    /// Make this group transparent - its children will be promoted to parent level
    /// Use this when you want logical grouping without creating nested output structures
    pub fn transparent(mut self) -> Self {
        self.group.transparent = true;
        self
    }

    /// Mark this group as metadata-only - it will extract metadata but not create
    /// a structure node in the hierarchy. Use this for global metadata field patterns
    /// like Section, MultiMic, etc. that should apply across all groups.
    pub fn metadata_only(mut self) -> Self {
        self.group.metadata_only = true;
        // Metadata-only groups should have very low priority so they don't interfere
        // with regular group matching, but still get checked for metadata extraction
        if self.group.priority == 0 {
            self.group.priority = -1000;
        }
        self
    }

    /// Require that the parent group also matches for this group to match.
    ///
    /// This is useful for groups like "Electronic Snare" that should only match
    /// when both the parent "Electronic Kit" and this "Snare" group match.
    ///
    /// # Example
    ///
    /// ```ignore
    /// Group::builder("Electronic Kit")
    ///     .group(
    ///         Group::builder("Snare")
    ///             .requires_parent_match()
    ///             .build()
    ///     )
    ///     .build()
    /// ```
    ///
    /// With this configuration, "Snare Top" will only match the "Snare" group
    /// if it also matches the "Electronic Kit" patterns (e.g., "electronic", "808", etc.).
    pub fn requires_parent_match(mut self) -> Self {
        self.group.requires_parent_match = true;
        self
    }

    /// Set the grouping strategy for a specific metadata field
    ///
    /// This allows different metadata fields to use different grouping behaviors.
    /// The order of fields in `metadata_fields` determines their priority for grouping.
    ///
    /// # Example
    ///
    /// ```ignore
    /// Group::builder("Guitar")
    ///     .field([ItemMetadataField::Arrangement, ItemMetadataField::MultiMic])
    ///     .field_strategy(ItemMetadataField::MultiMic, FieldGroupingStrategy::MainOnContainer)
    ///     .build()
    /// ```
    ///
    /// With this configuration:
    /// - Items are first grouped by Arrangement (priority 1)
    /// - Then by MultiMic (priority 2) using MainOnContainer strategy
    /// - Items without MultiMic go on the folder track
    /// - Items with MultiMic become children grouped by value
    pub fn field_strategy(mut self, field: M::Field, strategy: FieldGroupingStrategy) -> Self {
        let field_name = format!("{:?}", field);
        self.group
            .field_grouping_strategies
            .insert(field_name, strategy);
        self
    }

    /// Set the default value for a metadata field when items don't have that field
    ///
    /// When grouping by a metadata field, items without that field will be treated
    /// as if they have the field with the default value. This allows items without
    /// a field to be grouped alongside items that do have the field.
    ///
    /// # Example
    ///
    /// ```ignore
    /// Group::builder("Electric Guitar")
    ///     .field_default_value(ItemMetadataField::Layers, "Main")
    ///     .build()
    /// ```
    ///
    /// This means:
    /// - "Guitar Clean" (no layer) → treated as "Guitar Clean" with layer "Main"
    /// - "Guitar Clean DBL" (layer: "DBL") → grouped alongside "Main" items
    pub fn field_default_value(
        mut self,
        field: M::Field,
        default_value: impl Into<String>,
    ) -> Self {
        let field_name = format!("{:?}", field);
        self.group
            .field_default_values
            .insert(field_name, default_value.into());
        self
    }

    /// Build the final Group
    pub fn build(self) -> Group<M> {
        self.group
    }
}

// No longer need GroupProvider trait - use Into<Group<M>> instead
