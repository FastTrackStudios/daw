use crate::{Group, Metadata};
use facet::Facet;
use serde::{Deserialize, Serialize};

/// Configuration for the monarchy sorting system
#[derive(Clone, Debug, Serialize, Deserialize, Facet)]
pub struct Config<M: Metadata> {
    /// Groups that define the sorting patterns
    pub groups: Vec<Group<M>>,

    /// Rules for parsing input strings
    pub parser_rules: ParserRules,

    /// Strategy for handling unmatched items
    pub fallback_strategy: FallbackStrategy,
}

/// Rules for parsing input strings into metadata
#[derive(Clone, Debug, Serialize, Deserialize, Facet)]
pub struct ParserRules {
    /// Delimiters to use when splitting input strings
    pub delimiters: Vec<String>,

    /// Whether to preserve original casing
    pub preserve_case: bool,
}

impl Default for ParserRules {
    fn default() -> Self {
        Self {
            delimiters: vec![String::from(" "), String::from("_"), String::from("-")],
            preserve_case: false,
        }
    }
}

/// Strategy for handling items that don't match any group
#[repr(u8)]
#[derive(Clone, Debug, Serialize, Deserialize, Facet)]
pub enum FallbackStrategy {
    /// Create a "Misc" group for unmatched items
    CreateMisc,

    /// Place unmatched items at the root level
    PlaceAtRoot,

    /// Reject unmatched items with an error
    Reject,
}

impl<M: Metadata> Config<M> {
    /// Start building a new config - this is the only way to create a Config
    ///
    /// Default fallback strategy is `CreateMisc` - unmatched items will be placed in a "Misc" group.
    pub fn builder() -> ConfigBuilder<M> {
        ConfigBuilder {
            groups: Vec::new(),
            parser_rules: ParserRules::default(),
            fallback_strategy: FallbackStrategy::CreateMisc,
        }
    }
}

/// Builder for creating Config - the only way to create a Config
#[derive(Clone, Debug, Facet)]
pub struct ConfigBuilder<M: Metadata> {
    groups: Vec<Group<M>>,
    parser_rules: ParserRules,
    fallback_strategy: FallbackStrategy,
}

impl<M: Metadata> ConfigBuilder<M> {
    /// Add a group to the config
    pub fn group<G>(mut self, group: G) -> Self
    where
        G: Into<Group<M>>,
    {
        self.groups.push(group.into());
        self
    }

    /// Add multiple groups at once
    pub fn groups<G>(mut self, groups: impl IntoIterator<Item = G>) -> Self
    where
        G: Into<Group<M>>,
    {
        self.groups.extend(groups.into_iter().map(Into::into));
        self
    }

    /// Set the parser delimiters
    pub fn delimiters(mut self, delimiters: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.parser_rules.delimiters = delimiters.into_iter().map(|d| d.into()).collect();
        self
    }

    /// Set whether to preserve case during parsing
    pub fn preserve_case(mut self, preserve: bool) -> Self {
        self.parser_rules.preserve_case = preserve;
        self
    }

    /// Set the fallback strategy for unmatched items
    pub fn fallback(mut self, strategy: FallbackStrategy) -> Self {
        self.fallback_strategy = strategy;
        self
    }

    /// Build the final Config
    pub fn build(self) -> Config<M> {
        Config {
            groups: self.groups,
            parser_rules: self.parser_rules,
            fallback_strategy: self.fallback_strategy,
        }
    }
}
