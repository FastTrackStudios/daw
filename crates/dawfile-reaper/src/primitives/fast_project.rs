//! Fast line-scanned project parser for large RPP files.
//!
//! This parser keeps token semantics from `parse_token_line` while replacing
//! recursive nom block walking with a single pass over lines and a block stack.

use super::block::{BlockType, RppBlock, RppBlockContent};
use super::project::RppProject;
use super::token::{parse_token_line, Token};

fn parse_hex_u8(s: &str) -> Option<u8> {
    u8::from_str_radix(s, 16).ok()
}

fn fast_classify_token(raw: &str) -> Token {
    if let Some(hex) = raw.strip_prefix("0x") {
        if let Ok(v) = u64::from_str_radix(hex, 16) {
            return Token::HexInteger(v);
        }
    }

    if !raw.contains('.') && !raw.contains(',') && !raw.contains('e') && !raw.contains('E') {
        if let Ok(v) = raw.parse::<i64>() {
            return Token::Integer(v);
        }
    }

    if raw.contains(',') {
        let normalized = raw.replace(',', ".");
        if let Ok(v) = normalized.parse::<f64>() {
            return Token::Float(v);
        }
    } else if let Ok(v) = raw.parse::<f64>() {
        return Token::Float(v);
    }

    Token::Identifier(raw.to_string())
}

fn tokenize_line(line: &str) -> Result<Vec<Token>, String> {
    if line.contains('"')
        || line.contains('\'')
        || line.contains('`')
        || line.contains('#')
        || line.contains(';')
    {
        return parse_token_line(line)
            .map(|(_, t)| t)
            .map_err(|e| format!("{e:?}"));
    }

    let mut parts = line.split_whitespace();
    let Some(first) = parts.next() else {
        return Err("empty token line".to_string());
    };

    // Fast MIDI event classification: E/e <time> <hex> <hex> <hex> [extra...]
    if first == "E" || first == "e" {
        let p1 = parts.next();
        let p2 = parts.next();
        let p3 = parts.next();
        let p4 = parts.next();
        if let (Ok(time), Some(b1), Some(b2), Some(b3)) = (
            p1.unwrap_or_default().parse::<i64>(),
            parse_hex_u8(p2.unwrap_or_default()),
            parse_hex_u8(p3.unwrap_or_default()),
            parse_hex_u8(p4.unwrap_or_default()),
        ) {
            let mut out = vec![Token::MidiEvent {
                time,
                bytes: [b1, b2, b3],
            }];
            for part in parts {
                out.push(fast_classify_token(part));
            }
            return Ok(out);
        }
    }

    let mut out = Vec::with_capacity(8);
    out.push(fast_classify_token(first));
    out.extend(parts.map(fast_classify_token));
    Ok(out)
}

fn parse_block_header(block_line: &str) -> Result<(String, Vec<Token>), String> {
    if block_line.is_empty() {
        return Err("empty block header".to_string());
    }

    if !block_line.contains(' ') && !block_line.contains('\t') {
        return Ok((block_line.to_string(), Vec::new()));
    }

    if block_line.contains('"')
        || block_line.contains('\'')
        || block_line.contains('`')
        || block_line.contains('#')
        || block_line.contains(';')
    {
        let tokens = tokenize_line(block_line)?;
        if tokens.is_empty() {
            return Err("empty block header".to_string());
        }
        let name = tokens[0].to_string();
        return Ok((name, tokens[1..].to_vec()));
    }

    let mut parts = block_line.split_whitespace();
    let Some(name) = parts.next() else {
        return Err("empty block header".to_string());
    };
    let mut params = Vec::with_capacity(4);
    params.extend(parts.map(fast_classify_token));
    Ok((name.to_string(), params))
}

fn requires_structured_tokens(block_name: &str) -> bool {
    matches!(
        block_name,
        "REAPER_PROJECT"
            | "TRACK"
            | "ITEM"
            | "TAKE"
            | "SOURCE"
            | "TEMPOENVEX"
            | "VOLENV"
            | "VOLENV2"
            | "PANENV"
            | "PANENV2"
            | "PARMENV"
    )
}

/// Parse a complete RPP project using a fast line scanner.
pub fn parse_rpp_fast(content: &str) -> Result<RppProject, String> {
    let mut lines = content.lines();
    let header_line = lines
        .by_ref()
        .find_map(|line| {
            let t = line.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.trim_start_matches('\u{feff}').to_string())
            }
        })
        .ok_or("empty input".to_string())?;

    if !header_line.starts_with("<REAPER_PROJECT") {
        return Err(format!(
            "expected <REAPER_PROJECT header, got: {}",
            header_line
        ));
    }

    let header_tail = header_line["<REAPER_PROJECT".len()..].trim();
    let header_tokens =
        tokenize_line(header_tail).map_err(|e| format!("failed to parse project header tokens: {e}"))?;

    let version = header_tokens
        .first()
        .and_then(|t| t.as_number())
        .unwrap_or(0.1);
    let version_string = header_tokens
        .get(1)
        .and_then(|t| t.as_string())
        .unwrap_or("")
        .to_string();
    let timestamp = header_tokens
        .get(2)
        .and_then(|t| t.as_number())
        .unwrap_or(0.0) as i64;

    let mut top_blocks: Vec<RppBlock> = Vec::new();
    let mut project_props = RppBlock {
        block_type: BlockType::Project,
        name: "REAPER_PROJECT".to_string(),
        params: vec![],
        children: vec![],
    };
    let mut stack: Vec<RppBlock> = Vec::new();
    let mut project_closed = false;

    for (line_idx0, raw_line) in lines.enumerate() {
        let line_no = line_idx0 + 2; // +1 for 1-based, +1 because header already consumed
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') || line.starts_with(';')
        {
            continue;
        }

        if line == ">" {
            if let Some(block) = stack.pop() {
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(RppBlockContent::Block(block));
                } else {
                    top_blocks.push(block);
                }
            } else {
                project_closed = true;
                break;
            }
            continue;
        }

        if line.starts_with('<') {
            let block_line = &line[1..];
            let (name, params) =
                parse_block_header(block_line).map_err(|e| format!("line {line_no}: invalid block header: {e}"))?;
            stack.push(RppBlock {
                block_type: BlockType::parse(&name),
                name,
                params,
                children: vec![],
            });
            continue;
        }

        let parse_structured = stack
            .last()
            .map(|b| requires_structured_tokens(&b.name))
            .unwrap_or(true);
        let tokens = if parse_structured {
            tokenize_line(line).map_err(|e| format!("line {line_no}: invalid content line: {e}"))?
        } else {
            vec![Token::Identifier(line.to_string())]
        };
        if let Some(parent) = stack.last_mut() {
            parent.children.push(RppBlockContent::Content(tokens));
        } else {
            project_props.children.push(RppBlockContent::Content(tokens));
        }
    }

    if !project_closed {
        return Err("missing closing > for REAPER_PROJECT".to_string());
    }
    if !stack.is_empty() {
        return Err(format!("unclosed blocks at EOF: {}", stack.len()));
    }

    if !project_props.children.is_empty() {
        top_blocks.insert(0, project_props);
    }

    Ok(RppProject {
        version,
        version_string,
        timestamp,
        blocks: top_blocks,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_rpp_fast;
    use crate::primitives::project::parse_rpp;

    #[test]
    fn fast_parser_matches_nom_small_project() {
        let input = r#"<REAPER_PROJECT 0.1 "6.75/linux-x86_64" 1681651369
  RIPPLE 0 0
  <TRACK
    NAME "Track 1"
    <ITEM
      POSITION 0
      LENGTH 1
    >
  >
>"#;

        let fast = parse_rpp_fast(input).expect("fast parse failed");
        let (rem, nom) = parse_rpp(input).expect("nom parse failed");
        assert!(rem.trim().is_empty(), "nom parser had trailing input");
        assert_eq!(fast, nom);
    }
}
