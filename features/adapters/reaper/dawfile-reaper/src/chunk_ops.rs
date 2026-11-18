//! Chunk text extraction and insertion helpers for REAPER state chunks.
//!
//! These functions operate on raw RPP chunk text (strings), enabling:
//! - Extracting FXCHAIN blocks from track state chunks
//! - Extracting named CONTAINER blocks from FX chains
//! - Inserting blocks into FX chains
//! - Wrapping FX chain content in a CONTAINER block
//!
//! Used by the module preset save/load flow to manipulate chunk text atomically.

/// Extract the FXCHAIN block from a track state chunk.
///
/// Returns the complete `<FXCHAIN ...>...</FXCHAIN>` text including delimiters,
/// or `None` if no FXCHAIN section is found.
pub fn extract_fxchain_block(track_chunk: &str) -> Option<&str> {
    extract_block_by_tag(track_chunk, "FXCHAIN")
}

/// Extract a named CONTAINER block from FX chain text.
///
/// Searches for `<CONTAINER Container "name"` (case-insensitive name match)
/// and returns the complete container block text including delimiters.
///
/// Returns `None` if no container with that name is found.
pub fn extract_container_block<'a>(fxchain_text: &'a str, container_name: &str) -> Option<&'a str> {
    let name_lower = container_name.to_lowercase();

    // Scan for <CONTAINER lines
    let bytes = fxchain_text.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Find next '<CONTAINER'
        if let Some(pos) = fxchain_text[i..].find("<CONTAINER") {
            let abs_pos = i + pos;

            // Check if this container's name matches
            let header_end = fxchain_text[abs_pos..]
                .find('\n')
                .map(|p| abs_pos + p)
                .unwrap_or(fxchain_text.len());
            let header = &fxchain_text[abs_pos..header_end];

            if container_name_matches(header, &name_lower) {
                // Found matching container — extract the full block
                if let Some(end) = find_block_end(fxchain_text, abs_pos) {
                    return Some(&fxchain_text[abs_pos..=end]);
                }
            }

            // Move past this <CONTAINER to look for the next one
            i = abs_pos + "<CONTAINER".len();
        } else {
            break;
        }
    }

    None
}

/// Insert a raw RPP chunk block into an FX chain.
///
/// The block (e.g., a `<CONTAINER ...>...>` block) is appended at the end
/// of the FX chain, before the closing `>` of the FXCHAIN section.
///
/// `track_chunk` is the complete track state chunk text.
/// Returns the modified track chunk text.
pub fn insert_into_fxchain(track_chunk: &str, block_text: &str) -> Result<String, String> {
    // Find the FXCHAIN block
    let fxchain_start = track_chunk
        .find("<FXCHAIN")
        .ok_or_else(|| "No FXCHAIN section found in track chunk".to_string())?;

    // Find the closing > of the FXCHAIN block
    let fxchain_end = find_block_end(track_chunk, fxchain_start)
        .ok_or_else(|| "FXCHAIN block is not properly closed".to_string())?;

    // The closing > is at fxchain_end. Insert the block text before it.
    // Find the line start of the closing >
    let close_line_start = track_chunk[..fxchain_end]
        .rfind('\n')
        .map(|p| p + 1)
        .unwrap_or(fxchain_end);

    // Get the indentation of the closing >
    let close_line = &track_chunk[close_line_start..=fxchain_end];
    let indent = &close_line[..close_line.len() - close_line.trim_start().len()];

    // Build the insertion: indent the block text to match the FX chain level
    let indented_block = indent_block(block_text.trim(), indent);

    let mut result = String::with_capacity(track_chunk.len() + indented_block.len() + 2);
    result.push_str(&track_chunk[..close_line_start]);
    result.push_str(&indented_block);
    result.push('\n');
    result.push_str(&track_chunk[close_line_start..]);

    Ok(result)
}

/// Wrap FX chain content in a CONTAINER block.
///
/// Given raw FX chain content (the inner content of an FXCHAIN, not the
/// FXCHAIN wrapper itself), wraps it in a named container block.
///
/// Returns the complete `<CONTAINER Container "name" ""\n...\n>` text.
pub fn wrap_in_container(fx_content: &str, container_name: &str) -> String {
    let mut result = String::new();
    result.push_str(&format!(
        "<CONTAINER Container \"{}\" \"\"\n",
        container_name
    ));
    result.push_str("  CONTAINER_CFG 2 2 2 0\n");
    result.push_str("  SHOW 0\n");
    result.push_str("  LASTSEL 0\n");
    result.push_str("  DOCKED 0\n");

    // Indent the FX content by 2 spaces
    for line in fx_content.lines() {
        if line.trim().is_empty() {
            result.push('\n');
        } else {
            result.push_str("  ");
            result.push_str(line);
            result.push('\n');
        }
    }

    result.push('>');
    result
}

/// Extract an FXCHAIN_REC (input FX chain) block from a track state chunk.
pub fn extract_input_fxchain_block(track_chunk: &str) -> Option<&str> {
    extract_block_by_tag(track_chunk, "FXCHAIN_REC")
}

/// Get the inner content of an FXCHAIN block (everything between the header line and closing >).
///
/// Strips the `<FXCHAIN ...` header line and the closing `>` line.
pub fn fxchain_inner_content(fxchain_block: &str) -> Option<&str> {
    let trimmed = fxchain_block.trim();

    // Find end of first line
    let first_newline = trimmed.find('\n')?;
    let after_header = &trimmed[first_newline + 1..];

    // Find the last > (closing the block)
    let last_close = after_header.rfind('>')?;

    // Return everything between header and closing >
    let inner = &after_header[..last_close];

    // Trim trailing whitespace/newlines before the >
    Some(inner.trim_end())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Extract a block by its tag name from RPP text.
/// Returns the full block text from `<TAG` through the matching `>`.
fn extract_block_by_tag<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let pattern = format!("<{}", tag);
    let start = text.find(&pattern)?;
    let end = find_block_end(text, start)?;
    Some(&text[start..=end])
}

/// Find the position of the closing `>` for a block starting at `start_pos`.
/// Handles nested blocks correctly by tracking depth.
///
/// Uses line-based parsing: only `<TAG` at the start of a trimmed line opens a block,
/// and only `>` as the sole content of a trimmed line closes a block.
/// This avoids false positives from `<` and `>` inside base64 data or hex strings.
fn find_block_end(text: &str, start_pos: usize) -> Option<usize> {
    let mut depth = 0;
    let mut pos = start_pos;

    for line in text[start_pos..].lines() {
        let trimmed = line.trim();
        let line_end = pos + line.len(); // position after this line's content

        if trimmed.starts_with('<') && !trimmed.starts_with("<<") {
            depth += 1;
        } else if trimmed == ">" {
            depth -= 1;
            if depth == 0 {
                // Return position of the > character itself
                let gt_pos = text[pos..line_end].rfind('>').unwrap() + pos;
                return Some(gt_pos);
            }
        }

        // +1 for the newline that .lines() strips
        pos = line_end + 1;
    }

    None
}

/// Check if a CONTAINER header line matches a given name (case-insensitive).
///
/// Header formats:
/// - `<CONTAINER Container "DRIVE" ""`
/// - `<CONTAINER Container DRIVE`
fn container_name_matches(header: &str, name_lower: &str) -> bool {
    // Try quoted name
    let mut in_quote = false;
    let mut quote_count = 0;
    let mut current_quoted = String::new();

    for ch in header.chars() {
        if ch == '"' {
            if in_quote {
                // Closing quote
                quote_count += 1;
                if quote_count == 1 {
                    // First quoted string — this is the container name in REAPER format
                    // (but the header is actually: Container "NAME" "")
                    // The first quoted string after "Container" is the name
                    if current_quoted.to_lowercase() == *name_lower {
                        return true;
                    }
                }
                current_quoted.clear();
                in_quote = false;
            } else {
                in_quote = true;
            }
        } else if in_quote {
            current_quoted.push(ch);
        }
    }

    // Fallback: check unquoted tokens
    let parts: Vec<&str> = header.split_whitespace().collect();
    // Typical: ["<CONTAINER", "Container", "NAME"] or ["<CONTAINER", "Container", "\"NAME\"", "\"\""]
    for part in parts.iter().skip(2) {
        let cleaned = part.trim_matches('"');
        if !cleaned.is_empty() && cleaned.to_lowercase() == *name_lower {
            return true;
        }
    }

    false
}

/// Indent all lines of a block by the given prefix.
fn indent_block(block: &str, indent: &str) -> String {
    block
        .lines()
        .map(|line| {
            if line.trim().is_empty() {
                String::new()
            } else {
                format!("{}{}", indent, line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
