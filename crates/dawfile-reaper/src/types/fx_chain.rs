//! FX chain data structures and parsing for REAPER

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::primitives::{BlockType, RppBlock};

/// A REAPER FX chain
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FxChain {
    pub name: String,
    pub plugins: Vec<Plugin>,
}

/// A plugin in an FX chain
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plugin {
    pub name: String,
    pub plugin_type: String,
    pub enabled: bool,
    pub parameters: Vec<f64>,
}

impl FxChain {
    /// Create an FxChain from a parsed RPP block
    pub fn from_block(block: &RppBlock) -> Result<Self, String> {
        if block.block_type != BlockType::FxChain {
            return Err(format!(
                "Expected FxChain block, got {:?}",
                block.block_type
            ));
        }

        let mut fx_chain = FxChain {
            name: String::new(),
            plugins: Vec::new(),
        };

        // Parse FX chain parameters from block content
        for child in &block.children {
            if let crate::primitives::RppBlockContent::Content(tokens) = child {
                if let Some(identifier) = tokens.first().and_then(|t| t.as_string()) {
                    match identifier {
                        "NAME" => {
                            if let Some(name_token) = tokens.get(1) {
                                fx_chain.name = name_token.as_string().unwrap_or("").to_string();
                            }
                        }
                        _ => {
                            // Ignore unknown parameters
                        }
                    }
                }
            }
        }

        // TODO: Parse nested plugins from block children
        // This would involve looking for nested blocks of type Plugin

        Ok(fx_chain)
    }
}

impl fmt::Display for FxChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "FX Chain: {}", self.name)?;
        writeln!(f, "  Plugins: {}", self.plugins.len())?;
        Ok(())
    }
}

impl fmt::Display for Plugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Plugin: {}", self.name)?;
        writeln!(f, "  Type: {}, Enabled: {}", self.plugin_type, self.enabled)?;
        writeln!(f, "  Parameters: {}", self.parameters.len())?;
        Ok(())
    }
}
