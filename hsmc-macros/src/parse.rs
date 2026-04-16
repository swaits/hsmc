//! Parsing for the `statechart!` grammar.

use proc_macro2::Span;
use syn::parse::{Parse, ParseStream};
use syn::{braced, parenthesized, token, Expr, Ident, Token, Type};

pub struct StatechartInput {
    pub name: Ident,
    pub body: StateBody,
}

pub struct StateBody {
    /// Only present at root.
    pub context_ty: Option<Type>,
    pub event_ty: Option<Type>,
    pub terminate: Option<(Ident, Span)>,
    pub default_child: Option<(Ident, Span)>,
    pub default_span: Option<Span>, // to detect duplicates
    pub default_count: usize,
    pub terminate_count: usize,
    pub entries: Vec<Ident>,
    pub exits: Vec<Ident>,
    pub handlers: Vec<Handler>,
    pub children: Vec<StateDecl>,
    /// `during: fn_name(field_a, field_b);` declarations on this state. Each
    /// `During` names a free async function and the root-context fields it
    /// needs `&mut` access to. Field non-overlap is enforced by Rust's native
    /// split borrow on the generated `select` call site; the macro emits a
    /// clearer error when it can detect overlap at expansion time.
    pub durings: Vec<During>,
}

impl StateBody {
    fn new() -> Self {
        Self {
            context_ty: None,
            event_ty: None,
            terminate: None,
            default_child: None,
            default_span: None,
            default_count: 0,
            terminate_count: 0,
            entries: Vec::new(),
            exits: Vec::new(),
            handlers: Vec::new(),
            children: Vec::new(),
            durings: Vec::new(),
        }
    }
}

/// `during: <fn_ident> ( <field_ident>, ... ) ;` — an async activity scoped to
/// a state. Runs concurrently with other durings on the active state path,
/// the external event channel, and the next timer deadline. Dropped whenever
/// any other branch of the run-loop's `select` wins or the state transitions.
#[derive(Clone)]
pub struct During {
    /// Name of the free async function the macro will call.
    pub fn_name: Ident,
    /// Fields of the root context struct the function borrows `&mut`.
    /// An empty list means the function takes no arguments.
    pub fields: Vec<Ident>,
    /// Span of the `during:` keyword for diagnostic messages. Currently
    /// unused by codegen (per-field spans come from the ident list) but
    /// kept for future clearer diagnostics.
    #[allow(dead_code)]
    pub kw_span: Span,
}

pub struct StateDecl {
    pub name: Ident,
    pub body: StateBody,
    pub span: Span,
}

#[derive(Clone)]
pub struct Handler {
    pub trigger: Trigger,
    pub kind: HandlerKind,
    pub decl_index: usize,
}

#[derive(Clone)]
pub enum HandlerKind {
    /// Unresolved handler target: could be the name of a declared state
    /// (→ transition) or the name of an action handler function (→ internal
    /// transition). Classified during IR build once all state names are
    /// known.
    Target(Ident),
}

#[derive(Clone)]
pub enum Trigger {
    Event {
        variant: Ident,
        payload: Option<EventPayload>,
    },
    /// An arbitrary expression evaluating to `core::time::Duration`, together
    /// with a human-readable key for dedup/diagnostics. `repeat` means the
    /// timer re-arms after firing; only valid on action handlers.
    Duration {
        expr: Expr,
        key: String,
        span: Span,
        repeat: bool,
    },
}

/// Typed payload bindings attached to an event-variant trigger.
/// e.g. `action(PacketRx(rssi: i16, snr: i16)) => h;` parses to
/// `EventPayload { kind: Tuple, fields: [(rssi, i16), (snr, i16)] }`.
#[derive(Clone)]
pub struct EventPayload {
    pub kind: PayloadKind,
    pub fields: Vec<PayloadField>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PayloadKind {
    Tuple,
    Struct,
}

#[derive(Clone)]
pub struct PayloadField {
    pub name: Ident,
    pub ty: Type,
}

impl Trigger {
    #[allow(dead_code)]
    pub fn key(&self) -> String {
        match self {
            Trigger::Event { variant, .. } => format!("event:{}", variant),
            Trigger::Duration { key, repeat, .. } => format!("dur:{}:{}", key, repeat),
        }
    }
    #[allow(dead_code)]
    pub fn span(&self) -> Span {
        match self {
            Trigger::Event { variant, .. } => variant.span(),
            Trigger::Duration { span, .. } => *span,
        }
    }
    #[allow(dead_code)]
    pub fn display(&self) -> String {
        match self {
            Trigger::Event { variant, .. } => variant.to_string(),
            Trigger::Duration { key, .. } => key.clone(),
        }
    }
}

impl Parse for StatechartInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        let content;
        braced!(content in input);
        let body = parse_body(&content, true)?;
        Ok(StatechartInput { name, body })
    }
}

fn parse_body(input: ParseStream, is_root: bool) -> syn::Result<StateBody> {
    let mut body = StateBody::new();
    let mut decl_index: usize = 0;
    while !input.is_empty() {
        let lookahead = input.lookahead1();
        if lookahead.peek(Ident) {
            let ident: Ident = input.fork().parse()?;
            let name = ident.to_string();
            match name.as_str() {
                "context" => {
                    let _: Ident = input.parse()?;
                    input.parse::<Token![:]>()?;
                    let ty: Type = input.parse()?;
                    input.parse::<Token![;]>()?;
                    if !is_root {
                        return Err(syn::Error::new(ident.span(), "`context:` is only valid at root"));
                    }
                    body.context_ty = Some(ty);
                }
                "events" => {
                    let _: Ident = input.parse()?;
                    input.parse::<Token![:]>()?;
                    let ty: Type = input.parse()?;
                    input.parse::<Token![;]>()?;
                    if !is_root {
                        return Err(syn::Error::new(ident.span(), "`events:` is only valid at root"));
                    }
                    body.event_ty = Some(ty);
                }
                "entry" => {
                    let _: Ident = input.parse()?;
                    input.parse::<Token![:]>()?;
                    let list = parse_ident_list(input)?;
                    input.parse::<Token![;]>()?;
                    body.entries.extend(list);
                }
                "exit" => {
                    let _: Ident = input.parse()?;
                    input.parse::<Token![:]>()?;
                    let list = parse_ident_list(input)?;
                    input.parse::<Token![;]>()?;
                    body.exits.extend(list);
                }
                "default" => {
                    let kw: Ident = input.parse()?;
                    let inner;
                    parenthesized!(inner in input);
                    let child: Ident = inner.parse()?;
                    input.parse::<Token![;]>()?;
                    body.default_count += 1;
                    if body.default_child.is_none() {
                        body.default_child = Some((child, kw.span()));
                        body.default_span = Some(kw.span());
                    }
                }
                "terminate" => {
                    let kw: Ident = input.parse()?;
                    let inner;
                    parenthesized!(inner in input);
                    let ev: Ident = inner.parse()?;
                    input.parse::<Token![;]>()?;
                    body.terminate_count += 1;
                    if body.terminate.is_none() {
                        body.terminate = Some((ev, kw.span()));
                    }
                }
                "on" => {
                    // Unified handler declaration. The RHS may be a single
                    // target or a comma-separated list — every entry in the
                    // list fires atomically in declaration order. Targets
                    // disambiguate at IR-build time: ident names a declared
                    // state → transition; otherwise → handler fn.
                    let _: Ident = input.parse()?;
                    let inner;
                    parenthesized!(inner in input);
                    let trigger = parse_trigger(&inner)?;
                    input.parse::<Token![=>]>()?;
                    let targets = parse_ident_list(input)?;
                    input.parse::<Token![;]>()?;
                    for target in targets {
                        body.handlers.push(Handler {
                            trigger: trigger.clone(),
                            kind: HandlerKind::Target(target),
                            decl_index,
                        });
                        decl_index += 1;
                    }
                }
                "during" => {
                    // `during: fn_name ( field_a, field_b, ... ) ;` or
                    // `during: fn_name ;` (zero-arg). Fields name struct
                    // members of the root context; the macro emits
                    // `fn_name(&mut ctx.field_a, &mut ctx.field_b)` at the
                    // call site and Rust's split borrow verifies no overlap.
                    let kw: Ident = input.parse()?;
                    input.parse::<Token![:]>()?;
                    let fn_name: Ident = input.parse()?;
                    let fields = if input.peek(token::Paren) {
                        let inner;
                        parenthesized!(inner in input);
                        let mut fields = Vec::new();
                        while !inner.is_empty() {
                            fields.push(inner.parse::<Ident>()?);
                            if inner.peek(Token![,]) {
                                inner.parse::<Token![,]>()?;
                            } else {
                                break;
                            }
                        }
                        if !inner.is_empty() {
                            return Err(inner.error("unexpected tokens after field list"));
                        }
                        fields
                    } else {
                        Vec::new()
                    };
                    input.parse::<Token![;]>()?;
                    body.durings.push(During {
                        fn_name,
                        fields,
                        kw_span: kw.span(),
                    });
                }
                "state" => {
                    let _: Ident = input.parse()?;
                    let sname: Ident = input.parse()?;
                    let content;
                    braced!(content in input);
                    let cbody = parse_body(&content, false)?;
                    body.children.push(StateDecl { name: sname.clone(), body: cbody, span: sname.span() });
                }
                other => {
                    return Err(syn::Error::new(ident.span(), format!("unknown item `{}` in statechart body", other)));
                }
            }
        } else {
            return Err(lookahead.error());
        }
    }
    Ok(body)
}

fn parse_ident_list(input: ParseStream) -> syn::Result<Vec<Ident>> {
    let mut v = Vec::new();
    v.push(input.parse()?);
    while input.peek(Token![,]) {
        input.parse::<Token![,]>()?;
        v.push(input.parse()?);
    }
    Ok(v)
}

fn parse_trigger(input: ParseStream) -> syn::Result<Trigger> {
    // Duration triggers must be prefixed with an explicit mode keyword:
    //   `every <dur>` — repeating timer
    //   `after <dur>` — one-shot timer
    //
    // Everything else is an event trigger:
    //   `Variant`                  — unit
    //   `Variant { a: T, b: U }`   — struct-style payload binding
    //   `Variant(a: T, b: U)`      — tuple-style payload binding
    if input.peek(Ident) {
        let fork = input.fork();
        if let Ok(id) = fork.parse::<Ident>() {
            if id == "every" {
                let _: Ident = input.parse()?;
                return parse_duration_rest(input, true);
            }
            if id == "after" {
                let _: Ident = input.parse()?;
                return parse_duration_rest(input, false);
            }
        }
    }

    if !input.peek(Ident) || input.peek2(Token![::]) {
        return Err(input.error(
            "expected an event variant (`Name`, `Name(..)`, `Name { .. }`) or a \
             duration trigger prefixed with `after` or `every`",
        ));
    }

    let variant: Ident = input.parse()?;

    if input.is_empty() {
        return Ok(Trigger::Event {
            variant,
            payload: None,
        });
    }

    if input.peek(token::Brace) {
        let inner;
        braced!(inner in input);
        let fields = parse_typed_fields(&inner)?;
        return Ok(Trigger::Event {
            variant,
            payload: Some(EventPayload {
                kind: PayloadKind::Struct,
                fields,
            }),
        });
    }

    if input.peek(token::Paren) {
        let inner;
        parenthesized!(inner in input);
        let fields = parse_typed_fields(&inner)?;
        return Ok(Trigger::Event {
            variant,
            payload: Some(EventPayload {
                kind: PayloadKind::Tuple,
                fields,
            }),
        });
    }

    Err(input.error(
        "unexpected token after event variant name; \
         expected end of trigger, `(..)`, or `{ .. }`",
    ))
}

fn parse_typed_fields(input: ParseStream) -> syn::Result<Vec<PayloadField>> {
    let mut fields = Vec::new();
    while !input.is_empty() {
        let name: Ident = input.parse()?;
        input.parse::<Token![:]>()?;
        let ty: Type = input.parse()?;
        fields.push(PayloadField { name, ty });
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        } else {
            break;
        }
    }
    if !input.is_empty() {
        return Err(input.error("unexpected tokens after typed payload bindings"));
    }
    Ok(fields)
}

fn parse_duration_rest(input: ParseStream, repeat: bool) -> syn::Result<Trigger> {
    let expr: Expr = input.parse()?;
    let span = syn::spanned::Spanned::span(&expr);
    // Bare numeric literals are rejected: readers shouldn't have to remember
    // what unit a lone integer or float means. Always require an explicit
    // `core::time::Duration` expression (or the re-exported `hsmc::Duration`).
    if let Expr::Lit(lit) = &expr {
        match &lit.lit {
            syn::Lit::Int(_) | syn::Lit::Float(_) => {
                return Err(syn::Error::new(
                    span,
                    "bare numeric literals are not accepted as durations; \
                     use a typed expression like `Duration::from_secs(5)` or \
                     `Duration::from_millis(250)`",
                ));
            }
            _ => {}
        }
    }
    let key = expr_to_string(&expr);
    Ok(Trigger::Duration { expr, key, span, repeat })
}

fn expr_to_string(e: &Expr) -> String {
    // Token-stream-based canonical form. Good enough for dedup keying.
    let ts = quote::quote! { #e };
    ts.to_string().split_whitespace().collect()
}
