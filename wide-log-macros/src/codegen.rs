use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};

use crate::parse::{DurationOverride, EventOverride, JsonNode, KeyOverrides, LogOverride, Marker, Number};

pub fn generate(
    root: JsonNode,
    overrides: KeyOverrides,
    tokio: bool,
    uuid: bool,
) -> Result<TokenStream2, syn::Error> {
    let mut ctx = GenContext::new(overrides);
    ctx.walk(&root, &[])?;

    ctx.auto_add_duration(&root)?;
    ctx.auto_add_event(&root)?;

    ctx.validate()?;

    Ok(ctx.emit(tokio, uuid))
}

#[derive(Clone, Debug)]
struct KeyEntry {
    json_name: String,
    variant: String,
}

#[derive(Clone, Debug)]
struct PathEntry {
    dotted: String,
    segments: Vec<String>,
}

#[derive(Clone, Debug)]
struct DefaultEntry {
    segments: Vec<String>,
    value: DefaultValue,
}

#[derive(Clone, Debug)]
enum DefaultValue {
    Str(String),
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
}

struct GenContext {
    keys: Vec<KeyEntry>,
    key_index: std::collections::BTreeMap<String, usize>,
    paths: Vec<PathEntry>,
    path_index: std::collections::BTreeMap<String, usize>,
    defaults: Vec<DefaultEntry>,
    duration_segments: Vec<String>,
    has_duration_marker: bool,
    timestamp_segments: Vec<String>,
    id_segments: Vec<String>,
    has_event_key: bool,
    // Resolved built-in key strings (user override or default).
    // Log group (library-level — emitted as Key trait constants):
    builtin_log: String,
    builtin_level: String,
    builtin_message: String,
    // Event group (macro-level — used in enum building):
    builtin_event: String,
    builtin_event_id: String,
    builtin_event_timestamp: String,
    // Duration group (macro-level — used in enum building):
    builtin_duration: String,
    builtin_duration_total_ms: String,
}

impl GenContext {
    fn new(overrides: KeyOverrides) -> Self {
        let LogOverride { key, level, message } = overrides.log;
        let EventOverride { key: ev_key, id, timestamp } = overrides.event;
        let DurationOverride { key: dur_key, total_ms } = overrides.duration;
        Self {
            keys: Vec::new(),
            key_index: std::collections::BTreeMap::new(),
            paths: Vec::new(),
            path_index: std::collections::BTreeMap::new(),
            defaults: Vec::new(),
            duration_segments: Vec::new(),
            has_duration_marker: false,
            timestamp_segments: Vec::new(),
            id_segments: Vec::new(),
            has_event_key: false,
            builtin_log: key.unwrap_or_else(|| "log".to_string()),
            builtin_level: level.unwrap_or_else(|| "level".to_string()),
            builtin_message: message.unwrap_or_else(|| "message".to_string()),
            builtin_event: ev_key.unwrap_or_else(|| "event".to_string()),
            builtin_event_id: id.unwrap_or_else(|| "id".to_string()),
            builtin_event_timestamp: timestamp.unwrap_or_else(|| "timestamp".to_string()),
            builtin_duration: dur_key.unwrap_or_else(|| "duration".to_string()),
            builtin_duration_total_ms: total_ms.unwrap_or_else(|| "total_ms".to_string()),
        }
    }

    fn add_key(&mut self, name: &str) -> usize {
        if let Some(&idx) = self.key_index.get(name) {
            return idx;
        }
        let idx = self.keys.len();
        let variant = to_pascal_case(name);
        self.keys.push(KeyEntry {
            json_name: name.to_string(),
            variant,
        });
        self.key_index.insert(name.to_string(), idx);
        idx
    }

    fn add_path(&mut self, segments: &[String]) {
        let dotted = segments.join(".");
        if self.path_index.contains_key(&dotted) {
            return;
        }
        let idx = self.paths.len();
        self.paths.push(PathEntry {
            dotted,
            segments: segments.to_vec(),
        });
        self.path_index.insert(self.paths[idx].dotted.clone(), idx);
    }

    fn walk(&mut self, node: &JsonNode, path: &[String]) -> Result<(), syn::Error> {
        match node {
            JsonNode::Object(entries) => {
                if path.is_empty() {
                    for (k, v) in entries {
                        let mut p = path.to_vec();
                        p.push(k.clone());
                        self.walk(v, &p)?;
                    }
                } else {
                    self.add_key(&path[path.len() - 1]);
                    self.add_path(path);
                    for (k, v) in entries {
                        let mut p = path.to_vec();
                        p.push(k.clone());
                        self.walk(v, &p)?;
                    }
                }
            }
            JsonNode::Null => {
                self.add_key(&path[path.len() - 1]);
                self.add_path(path);
            }
            JsonNode::Bool(b) => {
                self.add_key(&path[path.len() - 1]);
                self.add_path(path);
                self.defaults.push(DefaultEntry {
                    segments: path.to_vec(),
                    value: DefaultValue::Bool(*b),
                });
            }
            JsonNode::Number(n) => {
                self.add_key(&path[path.len() - 1]);
                self.add_path(path);
                let dv = match n {
                    Number::Int(x) => DefaultValue::Int(*x),
                    Number::Uint(x) => DefaultValue::Uint(*x),
                    Number::Float(x) => DefaultValue::Float(*x),
                };
                self.defaults.push(DefaultEntry {
                    segments: path.to_vec(),
                    value: dv,
                });
            }
            JsonNode::Str(s) => {
                self.add_key(&path[path.len() - 1]);
                self.add_path(path);
                self.defaults.push(DefaultEntry {
                    segments: path.to_vec(),
                    value: DefaultValue::Str(s.clone()),
                });
            }
            JsonNode::Marker(m) => {
                self.add_key(&path[path.len() - 1]);
                self.add_path(path);
                match m {
                    Marker::Duration => {
                        self.has_duration_marker = true;
                        self.duration_segments = path.to_vec();
                    }
                    Marker::Counter => {}
                }
            }
        }
        Ok(())
    }

    fn auto_add_duration(&mut self, root: &JsonNode) -> Result<(), syn::Error> {
        match root {
            JsonNode::Object(entries) => {
                let has_duration = entries.iter().any(|(k, _)| *k == self.builtin_duration);
                if !has_duration {
                    self.add_duration_subtree(&self.builtin_duration_total_ms.clone());
                } else {
                    let duration_node = entries
                        .iter()
                        .find(|(k, _)| *k == self.builtin_duration)
                        .map(|(_, v)| v)
                        .unwrap();
                    self.resolve_duration_subtree(duration_node)?;
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    fn add_duration_subtree(&mut self, leaf: &str) {
        let duration_seg = self.builtin_duration.clone();
        let leaf_seg = leaf.to_string();
        self.add_key(&duration_seg);
        self.add_key(&leaf_seg);
        let duration_path = vec![duration_seg, leaf_seg];
        self.add_path(&[duration_path[0].clone()]);
        self.add_path(&duration_path);
        self.has_duration_marker = true;
        self.duration_segments = duration_path;
    }

    fn resolve_duration_subtree(&mut self, node: &JsonNode) -> Result<(), syn::Error> {
        match node {
            JsonNode::Object(entries) => {
                let duration_seg = self.builtin_duration.clone();
                self.add_key(&duration_seg);
                self.add_path(&[duration_seg.clone()]);

                if entries.is_empty() {
                    self.add_duration_subtree(&self.builtin_duration_total_ms.clone());
                    return Ok(());
                }

                if self.has_duration_marker {
                    for (k, v) in entries {
                        let p = vec![duration_seg.clone(), k.clone()];
                        self.walk(v, &p)?;
                    }
                    return Ok(());
                }

                let duration_marker_leaf = entries
                    .iter()
                    .find(|(_, v)| matches!(v, JsonNode::Marker(Marker::Duration)))
                    .map(|(k, _)| k.clone());

                if let Some(_leaf) = duration_marker_leaf {
                    for (k, v) in entries {
                        let p = vec![duration_seg.clone(), k.clone()];
                        self.walk(v, &p)?;
                    }
                    return Ok(());
                }

                let total_ms_str = self.builtin_duration_total_ms.clone();
                let total_ms_entry = entries.iter().find(|(k, _)| *k == total_ms_str);

                if total_ms_entry.is_some() {
                    for (k, v) in entries {
                        if *k == total_ms_str {
                            self.add_key(&total_ms_str);
                            self.add_path(&[duration_seg.clone(), total_ms_str.clone()]);
                            self.has_duration_marker = true;
                            self.duration_segments =
                                vec![duration_seg.clone(), total_ms_str.clone()];
                        } else {
                            let p = vec![duration_seg.clone(), k.clone()];
                            self.walk(v, &p)?;
                        }
                    }
                    return Ok(());
                }

                let non_duration_leaves: Vec<&String> = entries
                    .iter()
                    .filter(|(_, v)| !matches!(v, JsonNode::Marker(Marker::Duration)))
                    .map(|(k, _)| k)
                    .collect();

                if non_duration_leaves.len() == 1 {
                    let leaf = non_duration_leaves[0].clone();
                    self.add_duration_subtree(&leaf);
                    let other_entries: Vec<&(String, JsonNode)> =
                        entries.iter().filter(|(k, _)| *k != leaf).collect();
                    for (k, v) in other_entries {
                        let p = vec![duration_seg.clone(), k.clone()];
                        self.walk(v, &p)?;
                    }
                    return Ok(());
                }

                Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "duration object has multiple non-duration leaves and no duration! marker; \
                     specify exactly one duration! leaf, or use \"total_ms\": duration!",
                ))
            }
            _ => {
                self.add_duration_subtree(&self.builtin_duration_total_ms.clone());
                Ok(())
            }
        }
    }

    fn auto_add_event(&mut self, root: &JsonNode) -> Result<(), syn::Error> {
        match root {
            JsonNode::Object(entries) => {
                let has_event = entries.iter().any(|(k, _)| *k == self.builtin_event);
                if !has_event {
                    self.add_event_subtree();
                } else {
                    let event_node = entries
                        .iter()
                        .find(|(k, _)| *k == self.builtin_event)
                        .map(|(_, v)| v)
                        .unwrap();
                    self.resolve_event_subtree(event_node)?;
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }

    fn add_event_subtree(&mut self) {
        let event_seg = self.builtin_event.clone();
        let timestamp_seg = self.builtin_event_timestamp.clone();
        let id_seg = self.builtin_event_id.clone();
        self.add_key(&event_seg);
        self.add_key(&timestamp_seg);
        self.add_key(&id_seg);
        self.add_path(&[event_seg.clone()]);
        self.add_path(&[event_seg.clone(), timestamp_seg.clone()]);
        self.add_path(&[event_seg.clone(), id_seg.clone()]);
        self.timestamp_segments = vec![event_seg.clone(), timestamp_seg];
        self.id_segments = vec![event_seg, id_seg];
        self.has_event_key = true;
    }

    fn resolve_event_subtree(&mut self, node: &JsonNode) -> Result<(), syn::Error> {
        let event_seg = self.builtin_event.clone();
        let timestamp_str = self.builtin_event_timestamp.clone();
        let id_str = self.builtin_event_id.clone();
        match node {
            JsonNode::Object(entries) => {
                self.add_key(&event_seg);
                self.add_path(&[event_seg.clone()]);

                if entries.is_empty() {
                    self.add_event_subtree();
                    return Ok(());
                }

                let has_timestamp = entries.iter().any(|(k, _)| *k == timestamp_str);
                let has_id = entries.iter().any(|(k, _)| *k == id_str);

                if !has_timestamp {
                    self.add_key(&timestamp_str);
                    self.add_path(&[event_seg.clone(), timestamp_str.clone()]);
                    self.timestamp_segments = vec![event_seg.clone(), timestamp_str.clone()];
                } else {
                    let ts_node = entries
                        .iter()
                        .find(|(k, _)| *k == timestamp_str)
                        .map(|(_, v)| v)
                        .unwrap();
                    self.walk(ts_node, &[event_seg.clone(), timestamp_str.clone()])?;
                    self.timestamp_segments = vec![event_seg.clone(), timestamp_str.clone()];
                }

                if !has_id {
                    self.add_key(&id_str);
                    self.add_path(&[event_seg.clone(), id_str.clone()]);
                    self.id_segments = vec![event_seg.clone(), id_str.clone()];
                } else {
                    let id_node = entries
                        .iter()
                        .find(|(k, _)| *k == id_str)
                        .map(|(_, v)| v)
                        .unwrap();
                    self.walk(id_node, &[event_seg.clone(), id_str.clone()])?;
                    self.id_segments = vec![event_seg.clone(), id_str.clone()];
                }

                for (k, v) in entries {
                    if *k != timestamp_str && *k != id_str {
                        let p = vec![event_seg.clone(), k.clone()];
                        self.walk(v, &p)?;
                    }
                }

                self.has_event_key = true;
            }
            _ => {
                return Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "\"event\" key must be an object, e.g. \"event\": { \"timestamp\": null, \"id\": null }",
                ));
            }
        }
        Ok(())
    }

    fn validate(&self) -> Result<(), syn::Error> {
        if !self.has_duration_marker {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "internal error: no duration path was set",
            ));
        }
        if self.duration_segments.is_empty() {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "internal error: duration path is empty",
            ));
        }
        if self.timestamp_segments.is_empty() {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "internal error: timestamp path is empty",
            ));
        }
        if self.id_segments.is_empty() {
            return Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "internal error: id path is empty",
            ));
        }
        Ok(())
    }

    fn emit(&self, tokio: bool, uuid: bool) -> TokenStream2 {
        let enum_variants: Vec<syn::Ident> = self
            .keys
            .iter()
            .map(|k| format_ident!("{}", k.variant))
            .collect();
        let enum_strs: Vec<String> = self.keys.iter().map(|k| k.json_name.clone()).collect();
        let max_keys = self.keys.len();

        let _as_str_arms: Vec<TokenStream2> = enum_variants
            .iter()
            .zip(enum_strs.iter())
            .map(|(v, s)| quote! { EventKey::#v => #s })
            .collect();

        let duration_path_idents: Vec<TokenStream2> = self
            .duration_segments
            .iter()
            .map(|s| {
                let ident = format_ident!("{}", to_pascal_case(s));
                quote! { EventKey::#ident }
            })
            .collect();

        let timestamp_path_idents: Vec<TokenStream2> = self
            .timestamp_segments
            .iter()
            .map(|s| {
                let ident = format_ident!("{}", to_pascal_case(s));
                quote! { EventKey::#ident }
            })
            .collect();

        let id_path_idents: Vec<TokenStream2> = self
            .id_segments
            .iter()
            .map(|s| {
                let ident = format_ident!("{}", to_pascal_case(s));
                quote! { EventKey::#ident }
            })
            .collect();

        let resolve_arms: Vec<TokenStream2> = self
            .paths
            .iter()
            .map(|p| {
                let dotted = &p.dotted;
                let segs: Vec<TokenStream2> = p
                    .segments
                    .iter()
                    .map(|s| {
                        let ident = format_ident!("{}", to_pascal_case(s));
                        quote! { EventKey::#ident }
                    })
                    .collect();
                quote! { #dotted => &[#(#segs),*] }
            })
            .collect();

        let default_stmts: Vec<TokenStream2> = self
            .defaults
            .iter()
            .map(|d| {
                let segs: Vec<TokenStream2> = d
                    .segments
                    .iter()
                    .map(|s| {
                        let ident = format_ident!("{}", to_pascal_case(s));
                        quote! { EventKey::#ident }
                    })
                    .collect();
                let val = match &d.value {
                    DefaultValue::Str(s) => quote! { ::wide_log::Value::from(#s) },
                    DefaultValue::Bool(b) => quote! { ::wide_log::Value::from(#b) },
                    DefaultValue::Int(n) => quote! { ::wide_log::Value::from(#n) },
                    DefaultValue::Uint(n) => quote! { ::wide_log::Value::from(#n) },
                    DefaultValue::Float(n) => {
                        let nf = *n;
                        quote! { ::wide_log::Value::from(#nf) }
                    }
                };
                quote! {
                    event.add_path(&[#(#segs),*], #val);
                }
            })
            .collect();

        let enum_def = quote! {
            #[derive(Copy, Clone, PartialEq, Eq, Debug)]
            #[repr(u8)]
            pub enum EventKey {
                #(#enum_variants),*
            }
        };

        let all_key_idents: Vec<TokenStream2> = enum_variants
            .iter()
            .map(|v| quote! { EventKey::#v })
            .collect();

        let key_strs: Vec<&str> = self.keys.iter().map(|k| k.json_name.as_str()).collect();

        let builtin_log = self.builtin_log.as_str();
        let builtin_level = self.builtin_level.as_str();
        let builtin_message = self.builtin_message.as_str();

        let key_impl = quote! {
            impl ::wide_log::Key for EventKey {
                fn as_str(self) -> &'static str {
                    <Self as ::wide_log::Key>::KEY_STRS[self as usize]
                }
                const MAX_KEYS: usize = #max_keys;
                const KEYS: &'static [Self] = &[#(#all_key_idents),*];
                const KEY_STRS: &'static [&'static str] = &[#(#key_strs),*];
                fn as_index(self) -> usize { self as usize }
                const DURATION_PATH: &'static [Self] = &[#(#duration_path_idents),*];
                const TIMESTAMP_PATH: &'static [Self] = &[#(#timestamp_path_idents),*];
                const ID_PATH: &'static [Self] = &[#(#id_path_idents),*];
                const LOG_KEY: &'static str = #builtin_log;
                const LEVEL_KEY: &'static str = #builtin_level;
                const MESSAGE_KEY: &'static str = #builtin_message;
            }
        };

        let resolve_fn = quote! {
            #[inline(always)]
            pub fn __wl_resolve_path(path: &str) -> &'static [EventKey] {
                match path {
                    #(#resolve_arms,)*
                    _ => panic!("unknown wide-log key path: {path}"),
                }
            }
        };

        let thread_local = quote! {
            thread_local! {
                static CURRENT_EVENT: ::wide_log::ContextCell<::wide_log::WideEvent<EventKey>> =
                    const { ::wide_log::ContextCell::new() };

                static EMIT_BUF: ::std::cell::RefCell<::std::vec::Vec<u8>> =
                    const { ::std::cell::RefCell::new(::std::vec::Vec::new()) };
            }
        };

        let default_emit = quote! {
            fn default_emit(ev: &::wide_log::WideEvent<EventKey>) {
                EMIT_BUF.with(|buf| {
                    let mut buf = buf.borrow_mut();
                    buf.clear();
                    if ev.serialize_to(&mut *buf).is_ok() {
                        // Safety: our serializer only writes valid UTF-8.
                        let json = unsafe { ::std::string::String::from_utf8_unchecked(buf.split_off(0)) };
                        ::wide_log::stdout_emit::submit(json);
                    }
                });
            }
        };

        let guard_struct = quote! {
            pub struct WideLogGuard<F: FnOnce(&::wide_log::WideEvent<EventKey>) + Send + 'static> {
                inner: ::std::boxed::Box<::wide_log::ScopedGuard<EventKey, F>>,
                prev_ptr: *mut ::wide_log::WideEvent<EventKey>,
            }

            // SAFETY: The raw pointer `prev_ptr` is only accessed via the
            // thread-local `CURRENT_EVENT` cell, which is per-thread. When the
            // guard is moved across threads (in async), the task-local
            // `TASK_EVENT` moves with the task. The pointer is never
            // dereferenced from a different thread than the one that set it.
            unsafe impl<F: FnOnce(&::wide_log::WideEvent<EventKey>) + Send + 'static> Send
                for WideLogGuard<F> {}
        };

        let default_id_fn = quote! {
            ::std::boxed::Box::new(|| ::wide_log::__re_exports_core::ulid::Ulid::generate().to_string())
        };

        let builder_struct = quote! {
            pub struct WideLogGuardBuilder<
                F: FnOnce(&::wide_log::WideEvent<EventKey>) + Send + 'static = fn(&::wide_log::WideEvent<EventKey>),
            > {
                tz: ::wide_log::__re_exports_core::chrono_tz::Tz,
                id_fn: ::std::boxed::Box<dyn FnOnce() -> String + Send>,
                emit_fn: F,
            }
        };

        let builder_impl = quote! {
            impl WideLogGuardBuilder<fn(&::wide_log::WideEvent<EventKey>)> {
                fn new() -> Self {
                    Self {
                        tz: ::wide_log::__re_exports_core::chrono_tz::Tz::UTC,
                        id_fn: #default_id_fn,
                        emit_fn: default_emit,
                    }
                }
            }

            impl<F: FnOnce(&::wide_log::WideEvent<EventKey>) + Send + 'static>
                WideLogGuardBuilder<F>
            {
                pub fn with_timezone(mut self, tz: ::wide_log::__re_exports_core::chrono_tz::Tz) -> Self {
                    self.tz = tz;
                    self
                }

                pub fn with_id<NewF: FnOnce() -> String + Send + 'static>(
                    mut self,
                    f: NewF,
                ) -> Self {
                    self.id_fn = ::std::boxed::Box::new(f);
                    self
                }

                pub fn with_emit<NewF: FnOnce(&::wide_log::WideEvent<EventKey>) + Send + 'static>(
                    self,
                    emit_fn: NewF,
                ) -> WideLogGuardBuilder<NewF> {
                    WideLogGuardBuilder {
                        tz: self.tz,
                        id_fn: self.id_fn,
                        emit_fn,
                    }
                }

                pub fn build(self) -> WideLogGuard<F> {
                    let id_str = (self.id_fn)();
                    let mut inner = ::std::boxed::Box::new(
                        ::wide_log::ScopedGuard::new_with_tz(self.emit_fn, self.tz),
                    );
                    {
                        use ::std::ops::DerefMut;
                        let event: &mut ::wide_log::WideEvent<EventKey> = inner.deref_mut();
                        #(#default_stmts)*
                        event.add_path(<EventKey as ::wide_log::Key>::ID_PATH, id_str);
                    }
                    let ptr: *mut ::wide_log::WideEvent<EventKey> = {
                        use ::std::ops::Deref;
                        let guard_ref: &::wide_log::ScopedGuard<EventKey, F> = inner.deref();
                        guard_ref.deref() as *const _ as *mut _
                    };
                    let prev_ptr = CURRENT_EVENT.with(|c| c.replace(ptr));
                    WideLogGuard { inner, prev_ptr }
                }
            }
        };

        let builder_uuid = if uuid {
            quote! {
                impl<F: FnOnce(&::wide_log::WideEvent<EventKey>) + Send + 'static>
                    WideLogGuardBuilder<F>
                {
                    pub fn with_uuid(self) -> Self {
                        self.with_id(|| ::wide_log::__re_exports_uuid::uuid::Uuid::new_v4().to_string())
                    }
                }
            }
        } else {
            TokenStream2::new()
        };

        let guard_builder_fn = quote! {
            impl WideLogGuard<fn(&::wide_log::WideEvent<EventKey>)> {
                pub fn builder() -> WideLogGuardBuilder<fn(&::wide_log::WideEvent<EventKey>)> {
                    WideLogGuardBuilder::new()
                }
            }
        };

        let guard_drop = quote! {
            impl<F: FnOnce(&::wide_log::WideEvent<EventKey>) + Send + 'static> Drop for WideLogGuard<F> {
                fn drop(&mut self) {
                    CURRENT_EVENT.with(|c| c.restore(self.prev_ptr));
                }
            }
        };

        let current_fn = if tokio {
            quote! {
                #[inline(always)]
                pub fn current() -> Option<&'static mut ::wide_log::WideEvent<EventKey>> {
                    let ptr = if let Ok(p) = TASK_EVENT.try_with(|c| c.get_ptr()) {
                        p
                    } else {
                        ::std::ptr::null_mut()
                    };
                    if !ptr.is_null() {
                        return Some(unsafe { &mut *ptr });
                    }
                    let ptr = CURRENT_EVENT.with(|c| c.get_ptr());
                    if ptr.is_null() {
                        None
                    } else {
                        Some(unsafe { &mut *ptr })
                    }
                }
            }
        } else {
            quote! {
                #[inline(always)]
                pub fn current() -> Option<&'static mut ::wide_log::WideEvent<EventKey>> {
                    let ptr = CURRENT_EVENT.with(|c| c.get_ptr());
                    if ptr.is_null() {
                        None
                    } else {
                        Some(unsafe { &mut *ptr })
                    }
                }
            }
        };

        let tokio_code = if tokio {
            let task_local = quote! {
                ::wide_log::__re_exports::tokio::task_local! {
                    static TASK_EVENT: ::wide_log::ContextCell<::wide_log::WideEvent<EventKey>>;
                }
            };

            let scope_fns = quote! {
                pub async fn scope<F, E>(emit_fn: E, f: F) -> F::Output
                where
                    F: ::std::future::Future,
                    E: FnOnce(&::wide_log::WideEvent<EventKey>) + Send + 'static,
                {
                    let id_str = ::wide_log::__re_exports_core::ulid::Ulid::generate().to_string();
                    let mut inner = ::std::boxed::Box::new(
                        ::wide_log::ScopedGuard::new_with_tz(emit_fn, ::wide_log::__re_exports_core::chrono_tz::Tz::UTC),
                    );
                    {
                        use ::std::ops::DerefMut;
                        let event: &mut ::wide_log::WideEvent<EventKey> = inner.deref_mut();
                        #(#default_stmts)*
                        event.add_path(<EventKey as ::wide_log::Key>::ID_PATH, id_str);
                    }
                    let ptr: *mut ::wide_log::WideEvent<EventKey> = {
                        use ::std::ops::Deref;
                        let guard_ref: &::wide_log::ScopedGuard<EventKey, E> = inner.deref();
                        guard_ref.deref() as *const _ as *mut _
                    };
                    let cell = ::wide_log::ContextCell::new();
                    cell.replace(ptr);
                    let _inner = inner;
                    TASK_EVENT.scope(cell, f).await
                }

                pub async fn scope_default<F: ::std::future::Future>(f: F) -> F::Output {
                    scope(default_emit, f).await
                }
            };

            let middleware = quote! {
                use std::task::{Context, Poll};
                use std::pin::Pin;

                #[derive(Clone)]
                pub struct WideLogLayer;

                impl<S> ::wide_log::__re_exports::tower::Layer<S> for WideLogLayer {
                    type Service = WideLogMiddleware<S>;
                    fn layer(&self, inner: S) -> Self::Service {
                        WideLogMiddleware { inner }
                    }
                }

                #[derive(Clone)]
                pub struct WideLogMiddleware<S> {
                    inner: S,
                }

                impl<S, ReqBody, ResBody, Err> ::wide_log::__re_exports::tower::Service<ReqBody> for WideLogMiddleware<S>
                where
                    S: ::wide_log::__re_exports::tower::Service<ReqBody, Response = ResBody, Error = Err>,
                    S::Future: Send + 'static,
                    ResBody: Send + 'static,
                    Err: Send + 'static,
                {
                    type Response = ResBody;
                    type Error = Err;
                    type Future = WideLogFuture<ResBody, Err>;

                    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                        self.inner.poll_ready(cx)
                    }

                    fn call(&mut self, req: ReqBody) -> Self::Future {
                        let inner_fut = self.inner.call(req);
                        WideLogFuture::new(inner_fut)
                    }
                }

                pub struct WideLogFuture<ResBody, Err> {
                    inner: ::std::pin::Pin<::std::boxed::Box<dyn ::std::future::Future<Output = Result<ResBody, Err>> + Send>>,
                }

                impl<ResBody, Err> WideLogFuture<ResBody, Err>
                where
                    ResBody: Send + 'static,
                    Err: Send + 'static,
                {
                    fn new<F>(inner: F) -> Self
                    where
                        F: ::std::future::Future<Output = Result<ResBody, Err>> + Send + 'static,
                    {
                        Self {
                            inner: ::std::boxed::Box::pin(scope_default(async move { inner.await })),
                        }
                    }
                }

                impl<ResBody, Err> Future for WideLogFuture<ResBody, Err> {
                    type Output = Result<ResBody, Err>;

                    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                        self.inner.as_mut().poll(cx)
                    }
                }
            };

            quote! {
                #task_local
                #scope_fns
                #middleware
            }
        } else {
            TokenStream2::new()
        };

        let macros = quote! {
            #[macro_export]
            macro_rules! wl_set {
                ($path:literal, $value:expr) => {
                    if let Some(ev) = current() {
                        ev.add_path(__wl_resolve_path($path), $value);
                    }
                };
            }

            #[macro_export]
            macro_rules! wl_inc {
                ($path:literal) => {
                    if let Some(ev) = current() {
                        ev.inc_path(__wl_resolve_path($path));
                    }
                };
            }

            #[macro_export]
            macro_rules! wl_dec {
                ($path:literal) => {
                    if let Some(ev) = current() {
                        ev.dec_path(__wl_resolve_path($path));
                    }
                };
            }

            #[macro_export]
            macro_rules! wl_add {
                ($path:literal, $n:expr) => {
                    if let Some(ev) = current() {
                        ev.add_n_path(__wl_resolve_path($path), $n);
                    }
                };
            }

            #[macro_export]
            macro_rules! wl_null {
                ($path:literal) => {
                    if let Some(ev) = current() {
                        ev.add_path(__wl_resolve_path($path), ());
                    }
                };
            }

            #[macro_export]
            macro_rules! info {
                ($msg:literal) => {
                    if let Some(ev) = current() {
                        ev.append_log_entry_static("info", $msg);
                    }
                };
                ($fmt:literal, $($arg:tt)*) => {
                    if let Some(ev) = current() {
                        let mut buf = ::std::string::String::with_capacity(64);
                        let _ = ::std::fmt::Write::write_fmt(&mut buf, ::std::format_args!($fmt, $($arg)*));
                        ev.append_log_entry("info", &buf);
                    }
                };
            }

            #[macro_export]
            macro_rules! warn {
                ($msg:literal) => {
                    if let Some(ev) = current() {
                        ev.append_log_entry_static("warn", $msg);
                    }
                };
                ($fmt:literal, $($arg:tt)*) => {
                    if let Some(ev) = current() {
                        let mut buf = ::std::string::String::with_capacity(64);
                        let _ = ::std::fmt::Write::write_fmt(&mut buf, ::std::format_args!($fmt, $($arg)*));
                        ev.append_log_entry("warn", &buf);
                    }
                };
            }

            #[macro_export]
            macro_rules! error {
                ($msg:literal) => {
                    if let Some(ev) = current() {
                        ev.append_log_entry_static("error", $msg);
                    }
                };
                ($fmt:literal, $($arg:tt)*) => {
                    if let Some(ev) = current() {
                        let mut buf = ::std::string::String::with_capacity(64);
                        let _ = ::std::fmt::Write::write_fmt(&mut buf, ::std::format_args!($fmt, $($arg)*));
                        ev.append_log_entry("error", &buf);
                    }
                };
            }

            #[macro_export]
            macro_rules! debug {
                ($msg:literal) => {
                    if let Some(ev) = current() {
                        ev.append_log_entry_static("debug", $msg);
                    }
                };
                ($fmt:literal, $($arg:tt)*) => {
                    if let Some(ev) = current() {
                        let mut buf = ::std::string::String::with_capacity(64);
                        let _ = ::std::fmt::Write::write_fmt(&mut buf, ::std::format_args!($fmt, $($arg)*));
                        ev.append_log_entry("debug", &buf);
                    }
                };
            }

            #[macro_export]
            macro_rules! trace {
                ($msg:literal) => {
                    if let Some(ev) = current() {
                        ev.append_log_entry_static("trace", $msg);
                    }
                };
                ($fmt:literal, $($arg:tt)*) => {
                    if let Some(ev) = current() {
                        let mut buf = ::std::string::String::with_capacity(64);
                        let _ = ::std::fmt::Write::write_fmt(&mut buf, ::std::format_args!($fmt, $($arg)*));
                        ev.append_log_entry("trace", &buf);
                    }
                };
            }
        };

        quote! {
            #enum_def
            #key_impl
            #resolve_fn
            #thread_local
            #default_emit
            #guard_struct
            #builder_struct
            #builder_impl
            #builder_uuid
            #guard_builder_fn
            #guard_drop
            #current_fn
            #tokio_code
            #macros
        }
    }
}

fn to_pascal_case(name: &str) -> String {
    let mut result = String::new();
    for word in name.split(|c| c == '_' || c == '.') {
        if word.is_empty() {
            continue;
        }
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            result.extend(first.to_uppercase());
            result.extend(chars);
        }
    }
    if is_rust_keyword(&result) {
        result.push('_');
    }
    result
}

fn is_rust_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "try"
            | "union"
    )
}