use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};

use crate::parse::{JsonNode, Marker, Number};

pub fn generate(root: JsonNode, tokio: bool) -> Result<TokenStream2, syn::Error> {
    let mut ctx = GenContext::new();
    ctx.walk(&root, &[])?;

    ctx.auto_add_duration(&root)?;

    ctx.validate()?;

    Ok(ctx.emit(tokio))
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
}

impl GenContext {
    fn new() -> Self {
        Self {
            keys: Vec::new(),
            key_index: std::collections::BTreeMap::new(),
            paths: Vec::new(),
            path_index: std::collections::BTreeMap::new(),
            defaults: Vec::new(),
            duration_segments: Vec::new(),
            has_duration_marker: false,
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
                let has_duration = entries.iter().any(|(k, _)| k == "duration");
                if !has_duration {
                    self.add_duration_subtree("total_ms");
                } else {
                    let duration_node = entries
                        .iter()
                        .find(|(k, _)| k == "duration")
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
        let duration_seg = "duration".to_string();
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
                let duration_seg = "duration".to_string();
                self.add_key(&duration_seg);
                self.add_path(&[duration_seg.clone()]);

                if entries.is_empty() {
                    self.add_duration_subtree("total_ms");
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

                let total_ms_entry = entries
                    .iter()
                    .find(|(k, _)| k == "total_ms");

                if total_ms_entry.is_some() {
                    for (k, v) in entries {
                        if k == "total_ms" {
                            self.add_key("total_ms");
                            self.add_path(&[duration_seg.clone(), "total_ms".to_string()]);
                            self.has_duration_marker = true;
                            self.duration_segments =
                                vec![duration_seg.clone(), "total_ms".to_string()];
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
                    let other_entries: Vec<&(String, JsonNode)> = entries
                        .iter()
                        .filter(|(k, _)| *k != leaf)
                        .collect();
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
                self.add_duration_subtree("total_ms");
                Ok(())
            }
        }
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
        Ok(())
    }

    fn emit(&self, tokio: bool) -> TokenStream2 {
        let enum_variants: Vec<syn::Ident> = self
            .keys
            .iter()
            .map(|k| format_ident!("{}", k.variant))
            .collect();
        let enum_strs: Vec<String> = self.keys.iter().map(|k| k.json_name.clone()).collect();
        let max_keys = self.keys.len();

        let as_str_arms: Vec<TokenStream2> = enum_variants
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
                    inner.add_path(&[#(#segs),*], #val);
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

        let key_impl = quote! {
            impl ::wide_log::Key for EventKey {
                fn as_str(self) -> &'static str {
                    match self {
                        #(#as_str_arms,)*
                    }
                }
                const MAX_KEYS: usize = #max_keys;
                fn as_index(self) -> usize { self as usize }
                const DURATION_PATH: &'static [Self] = &[#(#duration_path_idents),*];
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
                static CURRENT_EVENT: ::std::cell::Cell<*mut ::wide_log::WideEvent<EventKey>> =
                    const { ::std::cell::Cell::new(::std::ptr::null_mut()) };
            }
        };

        let default_emit = quote! {
            fn default_emit(ev: &::wide_log::WideEvent<EventKey>) {
                if let Ok(json) = ev.to_json() {
                    ::tracing::info!(target: "wide_log", event = %json);
                }
            }
        };

        let guard_struct = quote! {
            pub struct EventKeyGuard<F: FnOnce(&::wide_log::WideEvent<EventKey>) + Send + 'static> {
                inner: ::std::boxed::Box<::wide_log::WideEventGuard<EventKey, F>>,
                prev_ptr: *mut ::wide_log::WideEvent<EventKey>,
            }
        };

        let guard_new = quote! {
            impl EventKeyGuard<fn(&::wide_log::WideEvent<EventKey>)> {
                pub fn new() -> Self {
                    Self::new_with_emit(default_emit)
                }
            }
        };

        let guard_new_with_emit = if default_stmts.is_empty() {
            quote! {
                impl<F: FnOnce(&::wide_log::WideEvent<EventKey>) + Send + 'static> EventKeyGuard<F> {
                    pub fn new_with_emit(emit_fn: F) -> Self {
                        let inner = ::std::boxed::Box::new(::wide_log::WideEventGuard::new(emit_fn));
                        let ptr: *mut ::wide_log::WideEvent<EventKey> = {
                            use ::std::ops::Deref;
                            inner.deref() as *const _ as *mut _
                        };
                        let prev_ptr = CURRENT_EVENT.with(|c| {
                            let prev = c.get();
                            c.set(ptr);
                            prev
                        });
                        Self { inner, prev_ptr }
                    }
                }
            }
        } else {
            quote! {
                impl<F: FnOnce(&::wide_log::WideEvent<EventKey>) + Send + 'static> EventKeyGuard<F> {
                    pub fn new_with_emit(emit_fn: F) -> Self {
                        let mut inner = ::std::boxed::Box::new(::wide_log::WideEventGuard::new(emit_fn));
                        {
                            use ::std::ops::DerefMut;
                            let event: &mut ::wide_log::WideEvent<EventKey> = inner.deref_mut();
                            #(#default_stmts)*
                        }
                        let ptr: *mut ::wide_log::WideEvent<EventKey> = {
                            use ::std::ops::Deref;
                            inner.deref() as *const _ as *mut _
                        };
                        let prev_ptr = CURRENT_EVENT.with(|c| {
                            let prev = c.get();
                            c.set(ptr);
                            prev
                        });
                        Self { inner, prev_ptr }
                    }
                }
            }
        };

        let guard_drop = quote! {
            impl<F: FnOnce(&::wide_log::WideEvent<EventKey>) + Send + 'static> Drop for EventKeyGuard<F> {
                fn drop(&mut self) {
                    CURRENT_EVENT.with(|c| {
                        c.set(self.prev_ptr);
                    });
                }
            }
        };

        let current_fn = if tokio {
            quote! {
                pub fn current() -> Option<&'static mut ::wide_log::WideEvent<EventKey>> {
                    if let Ok(ptr) = TASK_EVENT.try_with(|c| c.get()) {
                        if !ptr.is_null() {
                            return Some(unsafe { &mut *ptr });
                        }
                    }
                    CURRENT_EVENT.with(|c| {
                        let ptr = c.get();
                        if ptr.is_null() {
                            None
                        } else {
                            Some(unsafe { &mut *ptr })
                        }
                    })
                }
            }
        } else {
            quote! {
                pub fn current() -> Option<&'static mut ::wide_log::WideEvent<EventKey>> {
                    CURRENT_EVENT.with(|c| {
                        let ptr = c.get();
                        if ptr.is_null() {
                            None
                        } else {
                            Some(unsafe { &mut *ptr })
                        }
                    })
                }
            }
        };

        let tokio_code = if tokio {
            let task_local = quote! {
                ::wide_log::__re_exports::tokio::task_local! {
                    static TASK_EVENT: ::std::cell::Cell<*mut ::wide_log::WideEvent<EventKey>>;
                }
            };

            let scope_fns = quote! {
                pub async fn scope<F, E>(emit_fn: E, f: F) -> F::Output
                where
                    F: ::std::future::Future,
                    E: FnOnce(&::wide_log::WideEvent<EventKey>) + Send + 'static,
                {
                    let mut guard = EventKeyGuard::new_with_emit(emit_fn);
                    use ::std::ops::Deref;
                    let ptr: *mut ::wide_log::WideEvent<EventKey> =
                        guard.inner.deref() as *const _ as *mut _;
                    TASK_EVENT.scope(::std::cell::Cell::new(ptr), f).await
                }

                pub async fn scope_default<F: ::std::future::Future>(f: F) -> F::Output {
                    scope(default_emit, f).await
                }
            };

            let middleware = quote! {
                use std::task::{Context, Poll};
                use std::pin::Pin;
                use ::wide_log::__re_exports::tower::{Layer, Service};

                pub struct WideLogLayer;

                impl<S> Layer<S> for WideLogLayer {
                    type Service = WideLogMiddleware<S>;
                    fn layer(&self, inner: S) -> Self::Service {
                        WideLogMiddleware { inner }
                    }
                }

                pub struct WideLogMiddleware<S> {
                    inner: S,
                }

                impl<S, ReqBody, ResBody> Service<ReqBody> for WideLogMiddleware<S>
                where
                    S: Service<ReqBody, Response = ResBody>,
                {
                    type Response = S::Response;
                    type Error = S::Error;
                    type Future = WideLogFuture<S::Future>;

                    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                        self.inner.poll_ready(cx)
                    }

                    fn call(&mut self, req: ReqBody) -> Self::Future {
                        WideLogFuture {
                            inner: self.inner.call(req),
                        }
                    }
                }

                pub struct WideLogFuture<F> {
                    inner: F,
                }

                impl<F, ResBody, E> Future for WideLogFuture<F>
                where
                    F: Future<Output = Result<ResBody, E>>,
                {
                    type Output = Result<ResBody, E>;

                    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                        let inner = unsafe { self.map_unchecked_mut(|s| &mut s.inner) };
                        inner.poll(cx)
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
                        ev.append_log_entry("info", $msg);
                    }
                };
                ($fmt:literal, $($arg:tt)*) => {
                    if let Some(ev) = current() {
                        ev.append_log_entry("info", &format!($fmt, $($arg)*));
                    }
                };
            }

            #[macro_export]
            macro_rules! warn {
                ($msg:literal) => {
                    if let Some(ev) = current() {
                        ev.append_log_entry("warn", $msg);
                    }
                };
                ($fmt:literal, $($arg:tt)*) => {
                    if let Some(ev) = current() {
                        ev.append_log_entry("warn", &format!($fmt, $($arg)*));
                    }
                };
            }

            #[macro_export]
            macro_rules! error {
                ($msg:literal) => {
                    if let Some(ev) = current() {
                        ev.append_log_entry("error", $msg);
                    }
                };
                ($fmt:literal, $($arg:tt)*) => {
                    if let Some(ev) = current() {
                        ev.append_log_entry("error", &format!($fmt, $($arg)*));
                    }
                };
            }

            #[macro_export]
            macro_rules! debug {
                ($msg:literal) => {
                    if let Some(ev) = current() {
                        ev.append_log_entry("debug", $msg);
                    }
                };
                ($fmt:literal, $($arg:tt)*) => {
                    if let Some(ev) = current() {
                        ev.append_log_entry("debug", &format!($fmt, $($arg)*));
                    }
                };
            }

            #[macro_export]
            macro_rules! trace {
                ($msg:literal) => {
                    if let Some(ev) = current() {
                        ev.append_log_entry("trace", $msg);
                    }
                };
                ($fmt:literal, $($arg:tt)*) => {
                    if let Some(ev) = current() {
                        ev.append_log_entry("trace", &format!($fmt, $($arg)*));
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
            #guard_new
            #guard_new_with_emit
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
        "as" | "break" | "const" | "continue" | "crate" | "else" | "enum" | "extern"
        | "false" | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop" | "match"
        | "mod" | "move" | "mut" | "pub" | "ref" | "return" | "self" | "Self"
        | "static" | "struct" | "super" | "trait" | "true" | "type" | "unsafe"
        | "use" | "where" | "while" | "async" | "await" | "dyn" | "abstract"
        | "become" | "box" | "do" | "final" | "macro" | "override" | "priv" | "typeof"
        | "unsized" | "virtual" | "yield" | "try" | "union"
    )
}