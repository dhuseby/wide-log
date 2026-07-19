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

// ── Key override types ──

/// Override for the `Log` built-in key and its sub-keys.
#[derive(Debug, Clone, Default)]
pub struct LogOverride {
    /// The top-level "log" key itself. Override path: `Log`
    pub key: Option<String>,
    /// The "level" field within log entries. Override path: `Log.Level`
    pub level: Option<String>,
    /// The "message" field within log entries. Override path: `Log.Message`
    pub message: Option<String>,
}

/// Override for the `Event` built-in key and its sub-keys.
#[derive(Debug, Clone, Default)]
pub struct EventOverride {
    /// The top-level "event" key itself. Override path: `Event`
    pub key: Option<String>,
    /// The "id" field within the event object. Override path: `Event.Id`
    pub id: Option<String>,
    /// The "timestamp" field within the event object. Override path: `Event.Timestamp`
    pub timestamp: Option<String>,
}

/// Override for the `Duration` built-in key and its sub-keys.
#[derive(Debug, Clone, Default)]
pub struct DurationOverride {
    /// The top-level "duration" key itself. Override path: `Duration`
    pub key: Option<String>,
    /// The "total_ms" field within the duration object. Override path: `Duration.TotalMs`
    pub total_ms: Option<String>,
}

/// All user-specified key overrides. Fields are `None` when the user
/// does not override that key (default string will be used).
#[derive(Debug, Clone, Default)]
pub struct KeyOverrides {
    pub log: LogOverride,
    pub event: EventOverride,
    pub duration: DurationOverride,
}

/// Parses the full `wide_log!` input: an optional bracketed override list
/// followed by a JSON object. Returns the parsed overrides (or defaults)
/// and the JSON object node.
pub fn parse_wide_log_input(input: &TokenStream2) -> Result<(KeyOverrides, JsonNode), String> {
    let mut iter = input.clone().into_iter().peekable();

    // Peek: if the first token is a bracket group, parse overrides first.
    let overrides = if matches!(
        iter.peek(),
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Bracket
    ) {
        let ovr = parse_overrides(&mut iter)?;
        // Expect a comma separating the override list from the JSON object.
        match iter.next() {
            Some(TokenTree::Punct(p)) if p.as_char() == ',' => {}
            other => {
                return Err(format!(
                    "expected ',' after override list, got: {}",
                    other
                        .map(|t| t.to_string())
                        .unwrap_or("end of input".into())
                ));
            }
        }
        ovr
    } else {
        KeyOverrides::default()
    };

    let node = parse_value(&mut iter)?;
    if iter.peek().is_some() {
        return Err("unexpected trailing tokens after JSON value".into());
    }
    match node {
        JsonNode::Object(_) => Ok((overrides, node)),
        _ => Err("wide_log! expects a JSON object literal: { ... }".into()),
    }
}

/// Parses the bracketed override list: `[ Path => "str", ... ]`
fn parse_overrides(
    iter: &mut std::iter::Peekable<proc_macro2::token_stream::IntoIter>,
) -> Result<KeyOverrides, String> {
    let Some(TokenTree::Group(g)) = iter.next() else {
        return Err("expected override list in brackets".into());
    };
    if g.delimiter() != Delimiter::Bracket {
        return Err("override list must be in brackets [...]".into());
    }

    let mut ovr = KeyOverrides::default();
    let mut seen: BTreeMap<String, ()> = BTreeMap::new();
    let mut inner = g.stream().into_iter().peekable();

    if inner.peek().is_none() {
        return Err("override list is empty".into());
    }

    loop {
        // Parse dotted path: Ident (. Ident)?
        let path = parse_override_path(&mut inner)?;

        // Expect =>
        match inner.next() {
            Some(TokenTree::Punct(p)) if p.as_char() == '=' => {}
            other => {
                return Err(format!(
                    "expected '=>' after override path, got: {}",
                    other
                        .map(|t| t.to_string())
                        .unwrap_or("end of input".into())
                ));
            }
        }
        match inner.next() {
            Some(TokenTree::Punct(p)) if p.as_char() == '>' => {}
            other => {
                return Err(format!(
                    "expected '=>' after override path, got: {}",
                    other
                        .map(|t| t.to_string())
                        .unwrap_or("end of input".into())
                ));
            }
        }

        // Parse string literal
        let value = match inner.next() {
            Some(TokenTree::Literal(lit)) => {
                let s = lit.to_string();
                if s.starts_with('"') {
                    unescape_json_string(&s)?
                } else {
                    return Err(format!("override value must be a string literal, got: {s}"));
                }
            }
            other => {
                return Err(format!(
                    "override value must be a string literal, got: {}",
                    other
                        .map(|t| t.to_string())
                        .unwrap_or("end of input".into())
                ));
            }
        };

        // Check for duplicate
        let path_str = path.join(".");
        if seen.contains_key(&path_str) {
            return Err(format!("duplicate override path: {path_str}"));
        }
        seen.insert(path_str.clone(), ());

        // Assign to the correct field
        assign_override(&mut ovr, &path, &value)?;

        // Expect , or end
        match inner.next() {
            None => break,
            Some(TokenTree::Punct(p)) if p.as_char() == ',' => {
                if inner.peek().is_none() {
                    break;
                }
            }
            other => {
                return Err(format!(
                    "expected ',' or end of override list, got: {}",
                    other
                        .map(|t| t.to_string())
                        .unwrap_or("end of input".into())
                ));
            }
        }
    }

    Ok(ovr)
}

/// Parses a dotted override path: `Ident` or `Ident.Ident`
fn parse_override_path(
    iter: &mut std::iter::Peekable<proc_macro2::token_stream::IntoIter>,
) -> Result<Vec<String>, String> {
    let first = match iter.next() {
        Some(TokenTree::Ident(id)) => id.to_string(),
        other => {
            return Err(format!(
                "expected override path identifier, got: {}",
                other
                    .map(|t| t.to_string())
                    .unwrap_or("end of input".into())
            ));
        }
    };

    // Check for `.Ident` sub-key
    if let Some(TokenTree::Punct(p)) = iter.peek()
        && p.as_char() == '.'
    {
        iter.next(); // consume '.'
        let second = match iter.next() {
            Some(TokenTree::Ident(id)) => id.to_string(),
            other => {
                return Err(format!(
                    "expected sub-key identifier after '.', got: {}",
                    other
                        .map(|t| t.to_string())
                        .unwrap_or("end of input".into())
                ));
            }
        };
        return Ok(vec![first, second]);
    }

    Ok(vec![first])
}

/// Validates a user-supplied override string. Returns an error message
/// describing the problem if the value is invalid, or `None` if it is OK.
fn validate_override_value(path: &str, value: &str) -> Option<String> {
    if value.is_empty() {
        return Some(format!(
            "override for `{path}` is empty; this would produce \
             broken JSON output"
        ));
    }
    if value.contains('.') {
        return Some(format!(
            "override for `{path}` contains a `.` ({value:?}); \
             nested keys must be declared in the JSON body, not via override"
        ));
    }
    if value.contains('"') || value.contains('\\') {
        return Some(format!(
            "override for `{path}` contains a quote or backslash \
             ({value:?}); this would produce broken JSON output"
        ));
    }
    None
}

/// Assigns an override value to the correct field in `KeyOverrides`,
/// validating that the dotted path is a known built-in key path.
fn assign_override(ovr: &mut KeyOverrides, path: &[String], value: &str) -> Result<(), String> {
    let path_label = path.join(".");
    if let Some(msg) = validate_override_value(&path_label, value) {
        return Err(msg);
    }
    match path.len() {
        1 => match path[0].as_str() {
            "Log" => {
                if ovr.log.key.is_some() {
                    return Err("duplicate override for Log".into());
                }
                ovr.log.key = Some(value.to_string());
            }
            "Event" => {
                if ovr.event.key.is_some() {
                    return Err("duplicate override for Event".into());
                }
                ovr.event.key = Some(value.to_string());
            }
            "Duration" => {
                if ovr.duration.key.is_some() {
                    return Err("duplicate override for Duration".into());
                }
                ovr.duration.key = Some(value.to_string());
            }
            other => {
                return Err(format!(
                    "unknown override top-level key: '{other}' \
                     (expected Log, Event, or Duration)"
                ));
            }
        },
        2 => {
            let parent = &path[0];
            let sub = &path[1];
            match (parent.as_str(), sub.as_str()) {
                ("Log", "Level") => {
                    if ovr.log.level.is_some() {
                        return Err("duplicate override for Log.Level".into());
                    }
                    ovr.log.level = Some(value.to_string());
                }
                ("Log", "Message") => {
                    if ovr.log.message.is_some() {
                        return Err("duplicate override for Log.Message".into());
                    }
                    ovr.log.message = Some(value.to_string());
                }
                ("Event", "Id") => {
                    if ovr.event.id.is_some() {
                        return Err("duplicate override for Event.Id".into());
                    }
                    ovr.event.id = Some(value.to_string());
                }
                ("Event", "Timestamp") => {
                    if ovr.event.timestamp.is_some() {
                        return Err("duplicate override for Event.Timestamp".into());
                    }
                    ovr.event.timestamp = Some(value.to_string());
                }
                ("Duration", "TotalMs") => {
                    if ovr.duration.total_ms.is_some() {
                        return Err("duplicate override for Duration.TotalMs".into());
                    }
                    ovr.duration.total_ms = Some(value.to_string());
                }
                (parent, sub) => {
                    return Err(format!(
                        "unknown override path: '{parent}.{sub}' \
                         (valid paths: Log.Level, Log.Message, Event.Id, \
                         Event.Timestamp, Duration.TotalMs)"
                    ));
                }
            }
        }
        _ => {
            return Err(format!(
                "override path has too many segments: {} (max 2)",
                path.join(".")
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
pub fn parse_json_object(input: &TokenStream2) -> Result<JsonNode, String> {
    let mut iter = input.clone().into_iter().peekable();
    let node = parse_value(&mut iter)?;
    if iter.peek().is_some() {
        return Err("unexpected trailing tokens after JSON value".to_string());
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
                    let s = format!("-{lit}");
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
        let parsed = unescape_json_string(s)?;
        return Ok(JsonNode::Str(parsed));
    }

    if let Some(stripped) = s.strip_suffix("i64")
        && let Ok(n) = stripped.parse::<i64>()
    {
        return Ok(JsonNode::Number(Number::Int(n)));
    }
    if let Some(stripped) = s.strip_suffix("u64")
        && let Ok(n) = stripped.parse::<u64>()
    {
        return Ok(JsonNode::Number(Number::Uint(n)));
    }
    if let Some(stripped) = s.strip_suffix("f64")
        && let Ok(n) = stripped.parse::<f64>()
    {
        return Ok(JsonNode::Number(Number::Float(n)));
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

    // ── Override parsing tests ──

    fn parse_full(src: &str) -> (KeyOverrides, JsonNode) {
        let ts: TokenStream2 = src.parse().unwrap();
        parse_wide_log_input(&ts).unwrap()
    }

    fn parse_full_err(src: &str) -> String {
        let ts: TokenStream2 = src.parse().unwrap();
        parse_wide_log_input(&ts).unwrap_err()
    }

    #[test]
    fn no_overrides_backwards_compat() {
        let (ovr, _) = parse_full(r#"{ "status": null }"#);
        assert!(ovr.log.key.is_none());
        assert!(ovr.log.level.is_none());
        assert!(ovr.log.message.is_none());
        assert!(ovr.event.key.is_none());
        assert!(ovr.event.id.is_none());
        assert!(ovr.event.timestamp.is_none());
        assert!(ovr.duration.key.is_none());
        assert!(ovr.duration.total_ms.is_none());
    }

    #[test]
    fn all_overrides() {
        let (ovr, _) = parse_full(
            r#"[
                Log => "my_log",
                Log.Level => "severity",
                Log.Message => "msg",
                Event => "an_event",
                Event.Id => "correlation_id",
                Event.Timestamp => "ts",
                Duration => "the_duration",
                Duration.TotalMs => "how_long"
            ], { "service": null }"#,
        );
        assert_eq!(ovr.log.key.as_deref(), Some("my_log"));
        assert_eq!(ovr.log.level.as_deref(), Some("severity"));
        assert_eq!(ovr.log.message.as_deref(), Some("msg"));
        assert_eq!(ovr.event.key.as_deref(), Some("an_event"));
        assert_eq!(ovr.event.id.as_deref(), Some("correlation_id"));
        assert_eq!(ovr.event.timestamp.as_deref(), Some("ts"));
        assert_eq!(ovr.duration.key.as_deref(), Some("the_duration"));
        assert_eq!(ovr.duration.total_ms.as_deref(), Some("how_long"));
    }

    #[test]
    fn partial_overrides() {
        let (ovr, _) = parse_full(r#"[ Event.Id => "correlation_id" ], { "status": null }"#);
        assert!(ovr.event.key.is_none());
        assert_eq!(ovr.event.id.as_deref(), Some("correlation_id"));
        assert!(ovr.event.timestamp.is_none());
        assert!(ovr.log.key.is_none());
        assert!(ovr.duration.key.is_none());
    }

    #[test]
    fn top_level_only_override() {
        let (ovr, _) = parse_full(r#"[ Event => "an_event" ], { "status": null }"#);
        assert_eq!(ovr.event.key.as_deref(), Some("an_event"));
        assert!(ovr.event.id.is_none());
        assert!(ovr.event.timestamp.is_none());
    }

    #[test]
    fn err_unknown_top_level() {
        let err = parse_full_err(r#"[ Foo => "bar" ], { "status": null }"#);
        assert!(err.contains("unknown override top-level key"));
        assert!(err.contains("Foo"));
    }

    #[test]
    fn err_unknown_sub_key() {
        let err = parse_full_err(r#"[ Log.Bogus => "x" ], { "status": null }"#);
        assert!(err.contains("unknown override path"));
        assert!(err.contains("Log.Bogus"));
    }

    #[test]
    fn err_invalid_dotted_path_duration_id() {
        let err = parse_full_err(r#"[ Duration.Id => "x" ], { "status": null }"#);
        assert!(err.contains("unknown override path"));
        assert!(err.contains("Duration.Id"));
    }

    #[test]
    fn err_duplicate_override() {
        let err = parse_full_err(r#"[ Event => "a", Event => "b" ], { "status": null }"#);
        assert!(err.contains("duplicate override"));
    }

    #[test]
    fn err_missing_comma_after_overrides() {
        let err = parse_full_err(r#"[ Event => "a" ] { "status": null }"#);
        assert!(err.contains("expected ',' after override list"));
    }

    // ── Override validation tests (§4.5) ──

    #[test]
    fn err_empty_override_value() {
        let err = parse_full_err(r#"[ Log => "" ], { "status": null }"#);
        assert!(err.contains("empty"), "got: {err}");
        assert!(err.contains("Log"));
    }

    #[test]
    fn err_override_with_dot() {
        let err = parse_full_err(r#"[ Log => "a.b" ], { "status": null }"#);
        assert!(err.contains('.'), "got: {err}");
        assert!(err.contains("Log"));
    }

    #[test]
    fn err_subkey_override_with_dot() {
        let err = parse_full_err(r#"[ Log.Level => "x.y" ], { "status": null }"#);
        assert!(err.contains("Log.Level"), "got: {err}");
    }

    #[test]
    fn err_override_with_quote() {
        let err = parse_full_err(r#"[ Event => "a\"b" ], { "status": null }"#);
        assert!(err.contains("quote"), "got: {err}");
    }

    #[test]
    fn no_err_for_valid_override() {
        // Sanity: a clean override must not error.
        parse_full(r#"[ Log => "my_log" ], { "status": null }"#);
    }
}
