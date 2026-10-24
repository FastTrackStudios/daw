//! Generic RPP chunk tree API inspired by ReaTeam_RPP-Parser.
//!
//! This module provides a flexible line/chunk object model for:
//! - reading `.RPP` files or arbitrary REAPER chunk text
//! - traversing and mutating nodes/chunks
//! - writing projects/chunks back to text/files
//!
//! It is intentionally generic (string/token based) so it can support
//! projects, track/item chunks, FX chains (`<FXCHAIN>` / `.RfxChain` text),
//! and future format adapters.

use std::fmt;
use std::fs;
use std::path::Path;

use crate::{RppParseError, RppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GuidStripPolicy {
    /// Mirrors ReaTeam Lua `StripGUID` behavior.
    LuaCompat,
    /// Also strip additional IDs commonly needed for clone-safe workflow.
    Extended,
}

/// A token value in a node line.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RToken {
    pub token: String,
}

impl RToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }

    pub fn get_string(&self) -> &str {
        &self.token
    }

    pub fn get_number(&self) -> Option<f64> {
        self.token.parse::<f64>().ok()
    }

    /// In REAPER chunks, booleans are commonly represented as "0"/"1".
    pub fn get_boolean(&self) -> bool {
        self.token != "0"
    }

    pub fn set_string(&mut self, token: impl Into<String>) {
        self.token = token.into();
    }

    pub fn set_number(&mut self, token: f64) {
        self.token = token.to_string();
    }

    pub fn set_boolean(&mut self, value: bool) {
        self.token = if value { "1" } else { "0" }.to_string();
    }

    /// Quote a token if needed for REAPER-safe output.
    pub fn to_safe_string(s: &str) -> String {
        if s.is_empty() {
            "\"\"".to_string()
        } else if s.chars().any(|c| c.is_whitespace()) {
            if s.contains('"') {
                if s.contains('\'') {
                    format!("`{}`", s.replace('`', "'"))
                } else {
                    format!("'{}'", s)
                }
            } else {
                format!("\"{}\"", s)
            }
        } else {
            s.to_string()
        }
    }
}

impl fmt::Display for RToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.token)
    }
}

/// A non-chunk node line.
///
/// `tokens` has precedence over `line` during stringify, mirroring
/// ReaTeam parser behavior.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RNode {
    pub line: Option<String>,
    pub tokens: Option<Vec<RToken>>,
}

impl RNode {
    pub fn from_line(line: impl Into<String>) -> Self {
        Self {
            line: Some(line.into()),
            tokens: None,
        }
    }

    pub fn from_tokens(tokens: Vec<RToken>) -> Self {
        Self {
            line: None,
            tokens: Some(tokens),
        }
    }

    /// Lazy tokenization from line if needed.
    pub fn get_tokens(&mut self) -> &[RToken] {
        if self.tokens.is_none() {
            let toks = tokenize(self.line.as_deref().unwrap_or_default());
            self.tokens = Some(toks);
        }
        self.tokens.as_deref().unwrap_or(&[])
    }

    pub fn get_token(&mut self, index: usize) -> Option<&RToken> {
        self.get_tokens().get(index)
    }

    pub fn get_name(&mut self) -> Option<String> {
        self.get_token(0).map(|t| t.token.clone())
    }

    pub fn get_param(&mut self, index: usize) -> Option<String> {
        self.get_token(index + 1).map(|t| t.token.clone())
    }

    pub fn get_tokens_as_line(&mut self) -> String {
        self.get_tokens()
            .iter()
            .map(|t| RToken::to_safe_string(&t.token))
            .collect::<Vec<_>>()
            .join(" ")
    }

    // Lua-style compatibility wrappers
    #[allow(non_snake_case)]
    pub fn getTokens(&mut self) -> &[RToken] {
        self.get_tokens()
    }

    #[allow(non_snake_case)]
    pub fn getToken(&mut self, index: usize) -> Option<&RToken> {
        self.get_token(index)
    }

    #[allow(non_snake_case)]
    pub fn getName(&mut self) -> Option<String> {
        self.get_name()
    }

    #[allow(non_snake_case)]
    pub fn getParam(&mut self, index: usize) -> Option<String> {
        self.get_param(index)
    }

    #[allow(non_snake_case)]
    pub fn getTokensAsLine(&mut self) -> String {
        self.get_tokens_as_line()
    }

    /// Compatibility helper for Lua-style `node:remove()` semantics.
    ///
    /// Since the Rust tree does not store parent pointers yet, removal is
    /// performed against an explicit parent chunk.
    pub fn remove_from_parent(&self, parent: &mut RChunk) -> bool {
        if let Some(idx) = parent.children.iter().position(|child| match child {
            RNodeTree::Node(node) => node == self,
            RNodeTree::Chunk(_) => false,
        }) {
            parent.children.remove(idx);
            true
        } else {
            false
        }
    }

    #[allow(non_snake_case)]
    pub fn remove(&self, parent: &mut RChunk) -> bool {
        self.remove_from_parent(parent)
    }
}

/// A chunk tree child.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RNodeTree {
    Node(RNode),
    Chunk(RChunk),
}

impl RNodeTree {
    pub fn name(&self) -> Option<String> {
        match self {
            Self::Node(n) => {
                let mut clone = n.clone();
                clone.get_name()
            }
            Self::Chunk(c) => c.name(),
        }
    }
}

/// A chunk (`<...` ... `>`), which is also represented by a header node.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RChunk {
    pub header: RNode,
    pub children: Vec<RNodeTree>,
}

impl RChunk {
    fn in_lua_range(
        index_1_based: usize,
        start_index: Option<usize>,
        end_index: Option<usize>,
    ) -> bool {
        if let Some(start) = start_index
            && index_1_based < start
        {
            return false;
        }
        if let Some(end) = end_index
            && index_1_based > end
        {
            return false;
        }
        true
    }

    pub fn new(tokens: Vec<RToken>) -> Self {
        Self {
            header: RNode::from_tokens(tokens),
            children: Vec::new(),
        }
    }

    pub fn name(&self) -> Option<String> {
        let mut h = self.header.clone();
        h.get_name()
    }

    pub fn add_node(&mut self, node: RNodeTree) {
        self.children.push(node);
    }

    pub fn remove_node_at(&mut self, index: usize) -> Option<RNodeTree> {
        if index < self.children.len() {
            Some(self.children.remove(index))
        } else {
            None
        }
    }

    pub fn remove_node(&mut self, node: &RNodeTree) -> bool {
        if let Some(index) = self.index_of(node) {
            self.children.remove(index);
            true
        } else {
            false
        }
    }

    pub fn index_of(&self, node: &RNodeTree) -> Option<usize> {
        self.children.iter().position(|n| n == node)
    }

    pub fn find_first_node_by_name(&self, name: &str) -> Option<&RNode> {
        self.children.iter().find_map(|n| match n {
            RNodeTree::Node(node) => {
                let mut clone = node.clone();
                if clone.get_name().as_deref() == Some(name) {
                    Some(node)
                } else {
                    None
                }
            }
            RNodeTree::Chunk(_) => None,
        })
    }

    pub fn find_first_node_by_name_in_range(
        &self,
        name: &str,
        start_index: Option<usize>,
        end_index: Option<usize>,
    ) -> Option<&RNode> {
        self.children.iter().enumerate().find_map(|(i, n)| {
            let i = i + 1;
            if !Self::in_lua_range(i, start_index, end_index) {
                return None;
            }
            match n {
                RNodeTree::Node(node) => {
                    let mut clone = node.clone();
                    if clone.get_name().as_deref() == Some(name) {
                        Some(node)
                    } else {
                        None
                    }
                }
                RNodeTree::Chunk(_) => None,
            }
        })
    }

    pub fn find_first_chunk_by_name(&self, name: &str) -> Option<&RChunk> {
        self.children.iter().find_map(|n| match n {
            RNodeTree::Chunk(chunk) if chunk.name().as_deref() == Some(name) => Some(chunk),
            _ => None,
        })
    }

    pub fn find_first_chunk_by_name_in_range(
        &self,
        name: &str,
        start_index: Option<usize>,
        end_index: Option<usize>,
    ) -> Option<&RChunk> {
        self.children.iter().enumerate().find_map(|(i, n)| {
            let i = i + 1;
            if !Self::in_lua_range(i, start_index, end_index) {
                return None;
            }
            match n {
                RNodeTree::Chunk(chunk) if chunk.name().as_deref() == Some(name) => Some(chunk),
                _ => None,
            }
        })
    }

    pub fn find_all_nodes_by_name<'a>(&'a self, name: &str) -> Vec<&'a RNode> {
        self.children
            .iter()
            .filter_map(|n| match n {
                RNodeTree::Node(node) => {
                    let mut clone = node.clone();
                    if clone.get_name().as_deref() == Some(name) {
                        Some(node)
                    } else {
                        None
                    }
                }
                RNodeTree::Chunk(_) => None,
            })
            .collect()
    }

    pub fn find_all_nodes_by_name_in_range<'a>(
        &'a self,
        name: &str,
        start_index: Option<usize>,
        end_index: Option<usize>,
    ) -> Vec<&'a RNode> {
        self.children
            .iter()
            .enumerate()
            .filter_map(|(i, n)| {
                let i = i + 1;
                if !Self::in_lua_range(i, start_index, end_index) {
                    return None;
                }
                match n {
                    RNodeTree::Node(node) => {
                        let mut clone = node.clone();
                        if clone.get_name().as_deref() == Some(name) {
                            Some(node)
                        } else {
                            None
                        }
                    }
                    RNodeTree::Chunk(_) => None,
                }
            })
            .collect()
    }

    pub fn find_all_chunks_by_name<'a>(&'a self, name: &str) -> Vec<&'a RChunk> {
        self.children
            .iter()
            .filter_map(|n| match n {
                RNodeTree::Chunk(chunk) if chunk.name().as_deref() == Some(name) => Some(chunk),
                _ => None,
            })
            .collect()
    }

    pub fn find_all_chunks_by_name_in_range<'a>(
        &'a self,
        name: &str,
        start_index: Option<usize>,
        end_index: Option<usize>,
    ) -> Vec<&'a RChunk> {
        self.children
            .iter()
            .enumerate()
            .filter_map(|(i, n)| {
                let i = i + 1;
                if !Self::in_lua_range(i, start_index, end_index) {
                    return None;
                }
                match n {
                    RNodeTree::Chunk(chunk) if chunk.name().as_deref() == Some(name) => Some(chunk),
                    _ => None,
                }
            })
            .collect()
    }

    pub fn find_all_nodes_by_filter<F>(
        &self,
        filter: F,
        start_index: Option<usize>,
        end_index: Option<usize>,
    ) -> Vec<&RNodeTree>
    where
        F: Fn(&RNodeTree) -> bool,
    {
        self.children
            .iter()
            .enumerate()
            .filter_map(|(i, child)| {
                let i = i + 1;
                if !Self::in_lua_range(i, start_index, end_index) {
                    return None;
                }
                if filter(child) { Some(child) } else { None }
            })
            .collect()
    }

    pub fn find_all_chunks_by_filter<F>(
        &self,
        filter: F,
        start_index: Option<usize>,
        end_index: Option<usize>,
    ) -> Vec<&RChunk>
    where
        F: Fn(&RChunk) -> bool,
    {
        self.children
            .iter()
            .enumerate()
            .filter_map(|(i, child)| {
                let i = i + 1;
                if !Self::in_lua_range(i, start_index, end_index) {
                    return None;
                }
                match child {
                    RNodeTree::Chunk(chunk) if filter(chunk) => Some(chunk),
                    _ => None,
                }
            })
            .collect()
    }

    pub fn find_all_chunks_recursive<'a>(&'a self, name: &str, out: &mut Vec<&'a RChunk>) {
        for child in &self.children {
            if let RNodeTree::Chunk(chunk) = child {
                if chunk.name().as_deref() == Some(name) {
                    out.push(chunk);
                }
                chunk.find_all_chunks_recursive(name, out);
            }
        }
    }

    /// For `<NOTES ...>` style chunks, get newline-joined raw child lines.
    pub fn get_text_notes(&self) -> String {
        self.children
            .iter()
            .filter_map(|c| match c {
                RNodeTree::Node(n) => {
                    let raw = n.line.clone().unwrap_or_else(|| {
                        let mut clone = n.clone();
                        clone.get_tokens_as_line()
                    });
                    let raw = raw.trim_end();
                    let text = raw.strip_prefix('|').unwrap_or(raw).to_string();
                    Some(text)
                }
                RNodeTree::Chunk(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Replace child lines for `<NOTES ...>` style chunk text.
    pub fn set_text_notes(&mut self, text: &str) {
        self.children.clear();
        for line in text.lines() {
            self.children
                .push(RNodeTree::Node(RNode::from_line(format!("|{}", line))));
        }
    }

    /// Strip GUID-bearing lines recursively with explicit policy.
    pub fn strip_guid_with_policy(&mut self, policy: GuidStripPolicy) {
        self.children.retain(|child| match child {
            RNodeTree::Node(node) => {
                let mut clone = node.clone();
                !matches!(
                    (policy, clone.get_name().as_deref()),
                    (
                        GuidStripPolicy::LuaCompat,
                        Some("GUID" | "IGUID" | "TRACKID")
                    ) | (
                        GuidStripPolicy::Extended,
                        Some("GUID" | "IGUID" | "TRACKID" | "FXID" | "EGUID")
                    )
                )
            }
            RNodeTree::Chunk(_) => true,
        });
        for child in &mut self.children {
            if let RNodeTree::Chunk(chunk) = child {
                chunk.strip_guid_with_policy(policy);
            }
        }
    }

    /// Backward-compatible default stripping behavior (extended policy).
    pub fn strip_guid(&mut self) {
        self.strip_guid_with_policy(GuidStripPolicy::Extended)
    }

    // Lua-style compatibility wrappers
    #[allow(non_snake_case)]
    pub fn findFirstNodeByName(&self, name: &str) -> Option<&RNode> {
        self.find_first_node_by_name(name)
    }

    #[allow(non_snake_case)]
    pub fn findFirstNodeByNameInRange(
        &self,
        name: &str,
        start_index: Option<usize>,
        end_index: Option<usize>,
    ) -> Option<&RNode> {
        self.find_first_node_by_name_in_range(name, start_index, end_index)
    }

    #[allow(non_snake_case)]
    pub fn findFirstChunkByName(&self, name: &str) -> Option<&RChunk> {
        self.find_first_chunk_by_name(name)
    }

    #[allow(non_snake_case)]
    pub fn findFirstChunkByNameInRange(
        &self,
        name: &str,
        start_index: Option<usize>,
        end_index: Option<usize>,
    ) -> Option<&RChunk> {
        self.find_first_chunk_by_name_in_range(name, start_index, end_index)
    }

    #[allow(non_snake_case)]
    pub fn findAllNodesByName<'a>(&'a self, name: &str) -> Vec<&'a RNode> {
        self.find_all_nodes_by_name(name)
    }

    #[allow(non_snake_case)]
    pub fn findAllNodesByNameInRange<'a>(
        &'a self,
        name: &str,
        start_index: Option<usize>,
        end_index: Option<usize>,
    ) -> Vec<&'a RNode> {
        self.find_all_nodes_by_name_in_range(name, start_index, end_index)
    }

    #[allow(non_snake_case)]
    pub fn findAllChunksByName<'a>(&'a self, name: &str) -> Vec<&'a RChunk> {
        self.find_all_chunks_by_name(name)
    }

    #[allow(non_snake_case)]
    pub fn findAllChunksByNameInRange<'a>(
        &'a self,
        name: &str,
        start_index: Option<usize>,
        end_index: Option<usize>,
    ) -> Vec<&'a RChunk> {
        self.find_all_chunks_by_name_in_range(name, start_index, end_index)
    }

    #[allow(non_snake_case)]
    pub fn findAllNodesByFilter<F>(
        &self,
        filter: F,
        start_index: Option<usize>,
        end_index: Option<usize>,
    ) -> Vec<&RNodeTree>
    where
        F: Fn(&RNodeTree) -> bool,
    {
        self.find_all_nodes_by_filter(filter, start_index, end_index)
    }

    #[allow(non_snake_case)]
    pub fn findAllChunksByFilter<F>(
        &self,
        filter: F,
        start_index: Option<usize>,
        end_index: Option<usize>,
    ) -> Vec<&RChunk>
    where
        F: Fn(&RChunk) -> bool,
    {
        self.find_all_chunks_by_filter(filter, start_index, end_index)
    }

    #[allow(non_snake_case)]
    pub fn indexOf(&self, node: &RNodeTree) -> Option<usize> {
        self.index_of(node)
    }

    #[allow(non_snake_case)]
    pub fn getTextNotes(&self) -> String {
        self.get_text_notes()
    }

    #[allow(non_snake_case)]
    pub fn setTextNotes(&mut self, text: &str) {
        self.set_text_notes(text)
    }

    #[allow(non_snake_case)]
    pub fn addNode(&mut self, node: RNodeTree) {
        self.add_node(node)
    }

    #[allow(non_snake_case)]
    pub fn removeNodeAt(&mut self, index: usize) -> Option<RNodeTree> {
        self.remove_node_at(index)
    }

    #[allow(non_snake_case)]
    pub fn removeNode(&mut self, node: &RNodeTree) -> bool {
        self.remove_node(node)
    }

    #[allow(non_snake_case)]
    pub fn StripGUID(&mut self) {
        self.strip_guid_with_policy(GuidStripPolicy::LuaCompat)
    }

    /// Copy this chunk and insert the clone into `parent`.
    pub fn copy_to_parent(&self, parent: &mut RChunk) -> RChunk {
        let cloned = self.clone();
        parent.add_node(RNodeTree::Chunk(cloned.clone()));
        cloned
    }

    #[allow(non_snake_case)]
    pub fn copy(&self, parent: &mut RChunk) -> RChunk {
        self.copy_to_parent(parent)
    }
}

/// Create a root project chunk.
pub fn create_rpp(version: f64, system: &str, time: i64) -> RChunk {
    create_rchunk(vec![
        "REAPER_PROJECT".to_string(),
        version.to_string(),
        RToken::to_safe_string(system),
        time.to_string(),
    ])
}

pub fn create_rtokens(tokens: &[impl AsRef<str>]) -> Vec<RToken> {
    tokens
        .iter()
        .map(|t| RToken::new(t.as_ref().to_string()))
        .collect()
}

pub fn create_rchunk(tokens: Vec<String>) -> RChunk {
    RChunk::new(tokens.into_iter().map(RToken::new).collect())
}

pub fn create_rnode_from_line(line: impl Into<String>) -> RNode {
    RNode::from_line(line)
}

pub fn create_rnode_from_tokens(tokens: Vec<String>) -> RNode {
    RNode::from_tokens(tokens.into_iter().map(RToken::new).collect())
}

pub fn add_rchunk(parent: &mut RChunk, tokens: Vec<String>) {
    parent.add_node(RNodeTree::Chunk(create_rchunk(tokens)));
}

pub fn add_rnode(parent: &mut RChunk, tokens: Vec<String>) {
    parent.add_node(RNodeTree::Node(create_rnode_from_tokens(tokens)));
}

pub fn add_rtoken(node: &mut RNode, token: impl Into<String>) {
    if node.tokens.is_none() {
        node.tokens = Some(tokenize(node.line.as_deref().unwrap_or_default()));
    }
    node.tokens
        .get_or_insert_with(Vec::new)
        .push(RToken::new(token.into()));
}

/// Read a `.RPP` (or any single-root chunk file) from disk.
pub fn read_rpp(path: impl AsRef<Path>) -> RppResult<RChunk> {
    let content = fs::read_to_string(path)?;
    read_rpp_chunk(&content)
}

/// Parse chunk text into a single root chunk.
// r[impl rpp.parse.chunk-tree]
pub fn read_rpp_chunk(input: &str) -> RppResult<RChunk> {
    parse_root_chunk_lines(input.lines())
}

/// Parse chunk text from a list of lines.
pub fn read_rpp_lines<T: AsRef<str>>(lines: &[T]) -> RppResult<RChunk> {
    parse_root_chunk_lines(lines.iter().map(|l| l.as_ref()))
}

/// Stringify any node tree.
// r[impl rpp.write.chunk-tree]
pub fn stringify_rpp_node(node: &RNodeTree) -> String {
    stringify_node(node, 0)
}

/// Write root chunk back to file.
pub fn write_rpp(path: impl AsRef<Path>, root: &RChunk) -> RppResult<()> {
    fs::write(path, stringify_root(root))?;
    Ok(())
}

fn parse_root_chunk_lines<'a, I>(lines: I) -> RppResult<RChunk>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut stack: Vec<RChunk> = Vec::new();
    let mut root: Option<RChunk> = None;

    for (line_no, raw) in lines.into_iter().enumerate() {
        let line_no = line_no + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed == ">" {
            let done = stack.pop().ok_or_else(|| {
                RppParseError::ParseError(format!("line {line_no}: unexpected '>'"))
            })?;
            if let Some(parent) = stack.last_mut() {
                parent.children.push(RNodeTree::Chunk(done));
            } else if root.is_none() {
                root = Some(done);
            } else {
                return Err(RppParseError::ParseError(format!(
                    "line {line_no}: multiple root chunks encountered"
                )));
            }
            continue;
        }

        if let Some(after_lt) = trimmed.strip_prefix('<') {
            let tokens = tokenize(after_lt);
            if tokens.is_empty() {
                return Err(RppParseError::ParseError(format!(
                    "line {line_no}: empty chunk header"
                )));
            }
            stack.push(RChunk {
                // Keep the header's original text alongside its tokens, so an
                // untouched chunk renders back byte-for-byte. See `render_line`.
                header: RNode {
                    line: Some(after_lt.to_string()),
                    tokens: Some(tokens),
                },
                children: Vec::new(),
            });
            continue;
        }

        let tokens = tokenize(trimmed);
        let node = if tokens.is_empty() {
            RNode::from_line(trimmed.to_string())
        } else {
            RNode {
                line: Some(trimmed.to_string()),
                tokens: Some(tokens),
            }
        };

        if let Some(parent) = stack.last_mut() {
            parent.children.push(RNodeTree::Node(node));
        } else {
            return Err(RppParseError::ParseError(format!(
                "line {line_no}: cannot add node outside of chunk"
            )));
        }
    }

    if !stack.is_empty() {
        return Err(RppParseError::ParseError(
            "unterminated chunk(s): missing closing '>'".to_string(),
        ));
    }

    root.ok_or_else(|| RppParseError::ParseError("no root chunk found".to_string()))
}

fn stringify_root(root: &RChunk) -> String {
    let mut s = String::new();
    s.push_str(&stringify_node(&RNodeTree::Chunk(root.clone()), 0));
    if !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// Render one line, preferring the text it was parsed from.
///
/// A parsed node carries both its original `line` and the `tokens` that line
/// tokenizes to. Rendering from the tokens re-quotes them, which is
/// semantically identical but not byte-identical: REAPER writes
/// `FILE "Media/x.wav"`, and a token round-trip drops the now-unnecessary
/// quotes to `FILE Media/x.wav`. Harmless on its own — but it means a tool
/// that edits three lines of a project rewrites thousands, and no diff of its
/// output can be read.
///
/// So the original text wins whenever it still agrees with the tokens. Once
/// anything mutates the tokens they no longer tokenize back to `line`, and
/// rendering falls through to the tokens — which is what an edit means.
fn render_line(node: &RNode) -> String {
    match (&node.line, &node.tokens) {
        (Some(line), Some(tokens)) if tokenize(line) == *tokens => line.clone(),
        (_, Some(_)) => {
            let mut clone = node.clone();
            clone.get_tokens_as_line()
        }
        (Some(line), None) => line.clone(),
        (None, None) => String::new(),
    }
}

fn stringify_node(node: &RNodeTree, indent: usize) -> String {
    let pad = "  ".repeat(indent);
    match node {
        RNodeTree::Node(n) => {
            let line = render_line(n);
            format!("{pad}{line}")
        }
        RNodeTree::Chunk(c) => {
            let header_line = format!("<{}", render_line(&c.header));
            let mut out = format!("{pad}{header_line}");
            for child in &c.children {
                out.push('\n');
                out.push_str(&stringify_node(child, indent + 1));
            }
            out.push('\n');
            out.push_str(&pad);
            out.push('>');
            out
        }
    }
}

/// Tokenize a line using ReaTeam parser-compatible quoting rules:
/// - whitespace delimited
/// - quoted strings with `"`, `'`, or `` ` ``
pub fn tokenize(line: &str) -> Vec<RToken> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0usize;

    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }

        let mut buf = String::new();
        let c = chars[i];
        if c == '"' || c == '\'' || c == '`' {
            let quote = c;
            i += 1;
            while i < chars.len() {
                if chars[i] == quote {
                    i += 1;
                    break;
                }
                buf.push(chars[i]);
                i += 1;
            }
        } else {
            while i < chars.len() && !chars[i].is_whitespace() {
                buf.push(chars[i]);
                i += 1;
            }
        }

        tokens.push(RToken::new(buf));
    }

    tokens
}
