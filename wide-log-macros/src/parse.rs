use proc_macro2::{Delimiter, Literal, TokenStream as TokenStream2, TokenTree};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub enum JsonNode {
    Null,
    Bool(bool),
    Number(Number),
    Str(String),
    Object(Vec<(String, JsonNode)>),
    Marker(Marker),
}

#[derive(Debug, Clone)]
pub enum Number {
    Int(i64),
    Uint(u64),
    Float(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    Duration,
    Counter,
}

pub fn parse_json_object(input: &TokenStream2) -> Result<JsonNode, String> {
    let mut iter = input.clone().into_iter().peekable();
    let node = parse_value(&mut iter)?;
    if iter.peek().is_some() {
        return Err(format!("unexpected trailing tokens after JSON value"));
    }
    match node {
        JsonNode::Object(_) => Ok(node),
        _ => Err("wide_log! expects a JSON object literal: { ... }".into()),
    }
}

fn parse_value(
    iter: &mut std::iter::Peekable<proc_macro2::token_stream::IntoIter>,
) -> Result<JsonNode, String> {
    let Some(tt) = iter.next() else {
        return Err("unexpected end of input".into());
    };
    match tt {
        TokenTree::Ident(id) => {
            let s = id.to_string();
            match s.as_str() {
                "true" => Ok(JsonNode::Bool(true)),
                "false" => Ok(JsonNode::Bool(false)),
                "null" => Ok(JsonNode::Null),
                "duration" | "counter" => {
                    let marker = match s.as_str() {
                        "duration" => Marker::Duration,
                        "counter" => Marker::Counter,
                        _ => unreachable!(),
                    };
                    match iter.next() {
                        Some(TokenTree::Punct(p)) if p.as_char() == '!' => {
                            Ok(JsonNode::Marker(marker))
                        }
                        _ => Err(format!("expected '!' after '{s}' marker")),
                    }
                }
                other => Err(format!(
                    "unexpected identifier '{other}' (expected a JSON value)"
                )),
            }
        }
        TokenTree::Punct(p) if p.as_char() == '-' => {
            let next = iter.next().ok_or("expected literal after '-'")?;
            match next {
                TokenTree::Literal(lit) => {
                    let s = format!("-{}", lit);
                    parse_literal_str(&s)
                }
                other => Err(format!("expected literal after '-', got: {other}")),
            }
        }
        TokenTree::Literal(lit) => parse_literal(&lit),
        TokenTree::Group(g) if g.delimiter() == Delimiter::Brace => {
            let inner = g.stream();
            parse_object(&inner)
        }
        TokenTree::Group(g) if g.delimiter() == Delimiter::Parenthesis => {
            let inner = g.stream();
            let mut iter2 = inner.into_iter().peekable();
            let v = parse_value(&mut iter2)?;
            if iter2.peek().is_some() {
                return Err("unexpected trailing tokens inside parenthesized value".into());
            }
            Ok(v)
        }
        other => Err(format!("unexpected token: {other}")),
    }
}

fn parse_literal(lit: &Literal) -> Result<JsonNode, String> {
    parse_literal_str(&lit.to_string())
}

fn parse_literal_str(s: &str) -> Result<JsonNode, String> {
    if s.starts_with('"') {
        let parsed = unescape_json_string(&s)?;
        return Ok(JsonNode::Str(parsed));
    }

    if let Some(stripped) = s.strip_suffix("i64") {
        if let Ok(n) = stripped.parse::<i64>() {
            return Ok(JsonNode::Number(Number::Int(n)));
        }
    }
    if let Some(stripped) = s.strip_suffix("u64") {
        if let Ok(n) = stripped.parse::<u64>() {
            return Ok(JsonNode::Number(Number::Uint(n)));
        }
    }
    if let Some(stripped) = s.strip_suffix("f64") {
        if let Ok(n) = stripped.parse::<f64>() {
            return Ok(JsonNode::Number(Number::Float(n)));
        }
    }
    if let Ok(n) = s.parse::<u64>() {
        return Ok(JsonNode::Number(Number::Uint(n)));
    }
    if let Ok(n) = s.parse::<i64>() {
        return Ok(JsonNode::Number(Number::Int(n)));
    }
    if let Ok(n) = s.parse::<f64>() {
        return Ok(JsonNode::Number(Number::Float(n)));
    }

    Err(format!("cannot parse literal as a JSON value: {s}"))
}

fn parse_object(inner: &TokenStream2) -> Result<JsonNode, String> {
    let mut iter = inner.clone().into_iter().peekable();
    let mut entries: Vec<(String, JsonNode)> = Vec::new();
    let mut seen_keys: BTreeMap<String, usize> = BTreeMap::new();

    if iter.peek().is_none() {
        return Ok(JsonNode::Object(Vec::new()));
    }

    loop {
        let key = parse_key(&mut iter)?;
        match iter.next() {
            Some(TokenTree::Punct(p)) if p.as_char() == ':' => {}
            other => {
                return Err(format!(
                    "expected ':' after key, got: {}",
                    other
                        .map(|t| t.to_string())
                        .unwrap_or("end of input".into())
                ));
            }
        }
        let value = parse_value(&mut iter)?;
        if seen_keys.contains_key(&key) {
            return Err(format!("duplicate key \"{key}\" in wide_log! JSON"));
        }
        seen_keys.insert(key.clone(), entries.len());
        entries.push((key, value));

        match iter.next() {
            None => break,
            Some(TokenTree::Punct(p)) if p.as_char() == ',' => {
                if iter.peek().is_none() {
                    break;
                }
            }
            other => {
                return Err(format!(
                    "expected ',' or end of object, got: {}",
                    other
                        .map(|t| t.to_string())
                        .unwrap_or("end of input".into())
                ));
            }
        }
    }

    Ok(JsonNode::Object(entries))
}

fn parse_key(
    iter: &mut std::iter::Peekable<proc_macro2::token_stream::IntoIter>,
) -> Result<String, String> {
    let Some(tt) = iter.next() else {
        return Err("expected key in object".into());
    };
    match tt {
        TokenTree::Literal(lit) => {
            let s = lit.to_string();
            if s.starts_with('"') {
                unescape_json_string(&s)
            } else {
                Err(format!("object key must be a string literal, got: {s}"))
            }
        }
        TokenTree::Ident(id) => {
            let s = id.to_string();
            if s == "true" || s == "false" || s == "null" {
                Err(format!("invalid object key: {s}"))
            } else {
                Ok(s)
            }
        }
        other => Err(format!("object key must be a string literal, got: {other}")),
    }
}

fn unescape_json_string(raw: &str) -> Result<String, String> {
    let bytes = raw.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || bytes[bytes.len() - 1] != b'"' {
        return Err(format!("invalid string literal: {raw}"));
    }
    let inner = &raw[1..raw.len() - 1];
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        let esc = chars
            .next()
            .ok_or("unterminated escape in string literal")?;
        match esc {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            'n' => out.push('\n'),
            't' => out.push('\t'),
            'r' => out.push('\r'),
            '/' => out.push('/'),
            'b' => out.push('\u{0008}'),
            'f' => out.push('\u{000C}'),
            'u' => {
                let hex: String = (0..4).map(|_| chars.next().unwrap_or('0')).collect();
                let code = u32::from_str_radix(&hex, 16)
                    .map_err(|_| format!("invalid unicode escape: \\u{hex}"))?;
                out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
            }
            other => return Err(format!("invalid escape sequence: \\{other}")),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proc_macro2::TokenStream as TokenStream2;

    fn parse(src: &str) -> JsonNode {
        let ts: TokenStream2 = src.parse().unwrap();
        parse_json_object(&ts).unwrap()
    }

    #[test]
    fn parse_simple_object() {
        let n = parse(r#"{ "status": null }"#);
        match n {
            JsonNode::Object(entries) => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].0, "status");
                assert!(matches!(entries[0].1, JsonNode::Null));
            }
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn parse_nested_object() {
        let n = parse(r#"{ "service": { "name": null, "version": "1.0.0" } }"#);
        match n {
            JsonNode::Object(entries) => {
                assert_eq!(entries.len(), 1);
                match &entries[0].1 {
                    JsonNode::Object(inner) => {
                        assert_eq!(inner.len(), 2);
                        assert_eq!(inner[0].0, "name");
                        assert!(matches!(inner[0].1, JsonNode::Null));
                        assert_eq!(inner[1].0, "version");
                        assert!(matches!(&inner[1].1, JsonNode::Str(s) if s == "1.0.0"));
                    }
                    _ => panic!("expected nested object"),
                }
            }
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn parse_markers() {
        let n = parse(r#"{ "requests": counter!, "duration": { "total_ms": duration! } }"#);
        match n {
            JsonNode::Object(entries) => {
                assert_eq!(entries.len(), 2);
                assert!(matches!(&entries[0].1, JsonNode::Marker(Marker::Counter)));
                match &entries[1].1 {
                    JsonNode::Object(inner) => {
                        assert!(matches!(&inner[0].1, JsonNode::Marker(Marker::Duration)));
                    }
                    _ => panic!("expected nested object for duration"),
                }
            }
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn parse_numbers() {
        let n = parse(r#"{ "a": 42, "b": -7, "c": 3.14 }"#);
        match n {
            JsonNode::Object(entries) => {
                assert!(matches!(&entries[0].1, JsonNode::Number(Number::Uint(42))));
                assert!(matches!(&entries[1].1, JsonNode::Number(Number::Int(-7))));
                assert!(matches!(&entries[2].1, JsonNode::Number(Number::Float(_))));
            }
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn parse_bools() {
        let n = parse(r#"{ "flag": true, "off": false }"#);
        match n {
            JsonNode::Object(entries) => {
                assert!(matches!(&entries[0].1, JsonNode::Bool(true)));
                assert!(matches!(&entries[1].1, JsonNode::Bool(false)));
            }
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn parse_trailing_comma() {
        let n = parse(r#"{ "a": 1, "b": 2, }"#);
        match n {
            JsonNode::Object(entries) => assert_eq!(entries.len(), 2),
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn parse_empty_object() {
        let n = parse(r#"{ }"#);
        match n {
            JsonNode::Object(entries) => assert!(entries.is_empty()),
            _ => panic!("expected object"),
        }
    }

    #[test]
    fn parse_typed_numbers() {
        let n = parse(r#"{ "a": 42i64, "b": 7u64, "c": 1.5f64 }"#);
        match n {
            JsonNode::Object(entries) => {
                assert!(matches!(&entries[0].1, JsonNode::Number(Number::Int(42))));
                assert!(matches!(&entries[1].1, JsonNode::Number(Number::Uint(7))));
                assert!(matches!(&entries[2].1, JsonNode::Number(Number::Float(_))));
            }
            _ => panic!("expected object"),
        }
    }
}
