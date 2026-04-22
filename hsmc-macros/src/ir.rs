//! Intermediate representation built from the parsed AST.

use proc_macro2::Span;
use syn::{Expr, Ident, Type};

use crate::parse::{
    During, EventPayload, HandlerKind, PayloadField, PayloadKind, StateBody, StatechartInput,
    Trigger,
};

pub struct Ir {
    pub name: Ident,
    pub context_ty: Type,
    pub event_ty: Type,
    /// True if the user omitted `events:` (timer-only machine). In that case
    /// `event_ty` points to a synthesized empty enum and no `Sender` is emitted.
    pub events_omitted: bool,
    pub terminate_event: Option<Ident>,
    pub states: Vec<StateIr>,
    /// Action ident → signature info (trait-method index = position in vec).
    pub actions: Vec<ActionIr>,
    /// Event variants referenced anywhere, with the variant kind locked in
    /// on first use (unit / tuple / struct) for correct pattern matching in
    /// `__event_to_id`.
    pub event_variants: Vec<EventVariantIr>,
    /// Duration triggers referenced anywhere, in first-seen order.
    /// Each one gets a trigger id used by the timer table.
    pub duration_triggers: Vec<DurationTrigger>,
}

pub struct ActionIr {
    pub name: Ident,
    /// Typed parameters bound from an event payload. Empty for
    /// entry/exit/timer actions or actions triggered by unit-variant events.
    pub params: Vec<PayloadField>,
    /// For param-bearing actions, every variant this action is registered
    /// with (name + kind). Used by codegen to emit the dispatch match arm.
    pub bound_variants: Vec<BoundVariant>,
}

#[derive(Clone)]
pub struct BoundVariant {
    pub name: Ident,
    pub kind: PayloadKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VariantKind {
    Unit,
    Tuple,
    Struct,
}

pub struct EventVariantIr {
    pub name: Ident,
    pub kind: VariantKind,
    /// Number of fields in the variant (known only when the statechart
    /// declares a payload binding; `None` if seen only as unit).
    pub field_count: Option<usize>,
}

pub struct DurationTrigger {
    pub id: u16,
    pub key: String,
    pub expr: Expr,
    pub repeat: bool,
    pub span: Span,
}

pub struct StateIr {
    pub id: u16,
    pub name: Ident,
    pub parent: Option<u16>,
    pub depth: u8,
    pub default_child: Option<u16>,
    pub entries: Vec<u16>, // action ids
    pub exits: Vec<u16>,
    pub handlers: Vec<HandlerIr>,
    pub children: Vec<u16>,
    /// Duration triggers owned by this state (trigger id).
    pub owned_timers: Vec<u16>,
    /// `during:` activities declared on this state, preserved in declaration
    /// order. The macro emits these as concurrent `&mut`-field borrows in the
    /// run loop's `select` call for any state path that includes this state.
    pub durings: Vec<During>,
    pub span: Span,
}

pub struct HandlerIr {
    pub trigger: TriggerIr,
    pub kind: HandlerKindIr,
    pub decl_index: usize,
}

#[derive(Clone)]
pub enum TriggerIr {
    /// Event trigger. Payload is `Some` when the statechart declared typed
    /// bindings at this handler site; the bindings feed the action's typed
    /// handler signature.
    Event(u16, Ident, Option<EventPayload>),
    Duration(u16),
}

pub enum HandlerKindIr {
    Transition(Ident, Option<u16>),
    Action(u16, #[allow(dead_code)] Ident),
}

pub fn build_ir(input: StatechartInput) -> syn::Result<Ir> {
    let name = input.name;
    let root_body = input.body;

    let context_ty = root_body
        .context_ty
        .clone()
        .ok_or_else(|| syn::Error::new(name.span(), "statechart missing `context:` declaration"))?;
    let (event_ty, events_omitted) = match root_body.event_ty.clone() {
        Some(t) => (t, false),
        None => {
            let ty: Type = syn::parse_quote! { __NoEvents };
            (ty, true)
        }
    };
    let terminate_event = root_body.terminate.as_ref().map(|(i, _)| i.clone());

    let mut ir = Ir {
        name,
        context_ty,
        event_ty,
        events_omitted,
        terminate_event,
        states: Vec::new(),
        actions: Vec::new(),
        event_variants: Vec::new(),
        duration_triggers: Vec::new(),
    };

    // Two passes: collect the set of declared state names first so
    // `on(...) => Foo;` handlers can classify `Foo` as either a state
    // (transition) or a handler fn (action) based on the declared set.
    let mut state_names = std::collections::HashSet::new();
    collect_state_names(&root_body, &mut state_names);

    let name_span = ir.name.span();
    let root_name = Ident::new("__Root", name_span);
    let root_id = alloc_state(
        &mut ir,
        root_name,
        None,
        0,
        root_body.default_span.unwrap_or(name_span),
    );
    lower_body(&mut ir, root_id, &root_body, true, &state_names)?;

    Ok(ir)
}

fn collect_state_names(
    body: &crate::parse::StateBody,
    out: &mut std::collections::HashSet<String>,
) {
    for c in &body.children {
        out.insert(c.name.to_string());
        collect_state_names(&c.body, out);
    }
}

fn alloc_state(ir: &mut Ir, name: Ident, parent: Option<u16>, depth: u8, span: Span) -> u16 {
    let id = ir.states.len() as u16;
    ir.states.push(StateIr {
        id,
        name,
        parent,
        depth,
        default_child: None,
        entries: Vec::new(),
        exits: Vec::new(),
        handlers: Vec::new(),
        children: Vec::new(),
        owned_timers: Vec::new(),
        durings: Vec::new(),
        span,
    });
    id
}

fn intern_action_unit(ir: &mut Ir, i: &Ident) -> syn::Result<u16> {
    if let Some(p) = ir.actions.iter().position(|x| x.name == *i) {
        if !ir.actions[p].params.is_empty() {
            return Err(syn::Error::new(
                i.span(),
                format!(
                    "action `{}` was previously declared with typed event-payload \
                     bindings; cannot also be used without bindings",
                    i
                ),
            ));
        }
        return Ok(p as u16);
    }
    ir.actions.push(ActionIr {
        name: i.clone(),
        params: Vec::new(),
        bound_variants: Vec::new(),
    });
    Ok((ir.actions.len() - 1) as u16)
}

fn intern_action_with_params(
    ir: &mut Ir,
    i: &Ident,
    params: &[PayloadField],
    variant: &Ident,
    variant_kind: PayloadKind,
) -> syn::Result<u16> {
    if let Some(p) = ir.actions.iter().position(|x| x.name == *i) {
        if ir.actions[p].params.is_empty() {
            return Err(syn::Error::new(
                i.span(),
                format!(
                    "action `{}` was previously declared without bindings; \
                     cannot also be used with typed event-payload bindings",
                    i
                ),
            ));
        }
        if !params_match(&ir.actions[p].params, params) {
            return Err(syn::Error::new(
                i.span(),
                format!(
                    "action `{}` used with a different payload binding shape \
                     than its first declaration",
                    i
                ),
            ));
        }
        // Add this variant to bound_variants if not already present.
        let already = ir.actions[p]
            .bound_variants
            .iter()
            .any(|bv| bv.name == *variant);
        if !already {
            ir.actions[p].bound_variants.push(BoundVariant {
                name: variant.clone(),
                kind: variant_kind,
            });
        }
        return Ok(p as u16);
    }
    ir.actions.push(ActionIr {
        name: i.clone(),
        params: params.to_vec(),
        bound_variants: vec![BoundVariant {
            name: variant.clone(),
            kind: variant_kind,
        }],
    });
    Ok((ir.actions.len() - 1) as u16)
}

fn params_match(a: &[PayloadField], b: &[PayloadField]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).all(|(x, y)| {
        x.name == y.name && {
            let xs = {
                let t = &x.ty;
                quote::quote! { #t }.to_string()
            };
            let ys = {
                let t = &y.ty;
                quote::quote! { #t }.to_string()
            };
            xs == ys
        }
    })
}

fn intern_event(ir: &mut Ir, i: &Ident, payload: Option<&EventPayload>) -> syn::Result<u16> {
    let kind = match payload {
        None => VariantKind::Unit,
        Some(p) => match p.kind {
            PayloadKind::Tuple => VariantKind::Tuple,
            PayloadKind::Struct => VariantKind::Struct,
        },
    };
    let field_count = payload.map(|p| p.fields.len());

    if let Some(p) = ir.event_variants.iter().position(|x| x.name == *i) {
        let existing = &mut ir.event_variants[p];
        match (existing.kind, kind) {
            (VariantKind::Unit, VariantKind::Unit) => {}
            // First saw it as Unit, now a typed binding appears — upgrade
            // the variant's kind.
            (VariantKind::Unit, k) => {
                existing.kind = k;
                existing.field_count = field_count;
            }
            // Already knew it was tuple/struct, and this use is also typed.
            // Shape must match.
            (a, b) if a == b => {
                if let (Some(x), Some(y)) = (existing.field_count, field_count) {
                    if x != y {
                        return Err(syn::Error::new(
                            i.span(),
                            format!(
                                "event variant `{}` is used with inconsistent payload field counts ({} vs {})",
                                i, x, y
                            ),
                        ));
                    }
                }
            }
            // Already knew it was tuple/struct, this use is un-bound. Fine:
            // the user is matching the variant without extracting payload
            // (e.g. a transition target). The recorded kind controls the
            // match pattern in `__event_to_id`; leaving it as-is still
            // matches this use.
            (_, VariantKind::Unit) => {}
            // Conflict: tuple vs struct binding shape.
            (a, b) => {
                return Err(syn::Error::new(
                    i.span(),
                    format!(
                        "event variant `{}` is used with inconsistent kinds ({} vs {})",
                        i,
                        variant_kind_name(a),
                        variant_kind_name(b),
                    ),
                ));
            }
        }
        return Ok(p as u16);
    }

    ir.event_variants.push(EventVariantIr {
        name: i.clone(),
        kind,
        field_count,
    });
    Ok((ir.event_variants.len() - 1) as u16)
}

fn variant_kind_name(k: VariantKind) -> &'static str {
    match k {
        VariantKind::Unit => "unit",
        VariantKind::Tuple => "tuple",
        VariantKind::Struct => "struct",
    }
}

fn intern_duration(ir: &mut Ir, expr: &Expr, key: &str, repeat: bool, span: Span) -> u16 {
    let full_key = format!("{}:{}", key, repeat);
    if let Some(dt) = ir.duration_triggers.iter().find(|d| d.key == full_key) {
        return dt.id;
    }
    let id = ir.duration_triggers.len() as u16;
    ir.duration_triggers.push(DurationTrigger {
        id,
        key: full_key,
        expr: expr.clone(),
        repeat,
        span,
    });
    id
}

fn lower_body(
    ir: &mut Ir,
    state_id: u16,
    body: &StateBody,
    _is_root: bool,
    state_names: &std::collections::HashSet<String>,
) -> syn::Result<()> {
    let mut entries: Vec<u16> = Vec::new();
    for i in &body.entries {
        entries.push(intern_action_unit(ir, i)?);
    }
    let mut exits: Vec<u16> = Vec::new();
    for i in &body.exits {
        exits.push(intern_action_unit(ir, i)?);
    }
    ir.states[state_id as usize].entries = entries;
    ir.states[state_id as usize].exits = exits;

    let mut new_handlers = Vec::new();
    for h in &body.handlers {
        let trig = match &h.trigger {
            Trigger::Event { variant, payload } => {
                let id = intern_event(ir, variant, payload.as_ref())?;
                TriggerIr::Event(id, variant.clone(), payload.clone())
            }
            Trigger::Duration {
                expr,
                key,
                span,
                repeat,
            } => {
                let id = intern_duration(ir, expr, key, *repeat, *span);
                let owned = &mut ir.states[state_id as usize].owned_timers;
                if !owned.contains(&id) {
                    owned.push(id);
                }
                TriggerIr::Duration(id)
            }
        };
        let kind = match &h.kind {
            HandlerKind::Target(target) => classify_target(ir, target, &trig, state_names)?,
        };
        new_handlers.push(HandlerIr {
            trigger: trig,
            kind,
            decl_index: h.decl_index,
        });
    }
    ir.states[state_id as usize].handlers = new_handlers;

    // `during:` declarations are stored verbatim for codegen. Field-to-struct
    // validation happens in codegen (we don't have access to the context
    // struct's fields here) via the natural borrow-checker error on the
    // generated call site — validate.rs additionally catches the obvious
    // cases earlier.
    ir.states[state_id as usize].durings = body.durings.clone();

    let parent_depth = ir.states[state_id as usize].depth;
    let mut child_ids = Vec::new();
    for c in &body.children {
        let cid = alloc_state(ir, c.name.clone(), Some(state_id), parent_depth + 1, c.span);
        child_ids.push(cid);
        lower_body(ir, cid, &c.body, false, state_names)?;
    }
    ir.states[state_id as usize].children = child_ids.clone();

    if let Some((dc_ident, _)) = &body.default_child {
        let resolved = child_ids
            .iter()
            .find(|&&cid| ir.states[cid as usize].name == *dc_ident)
            .copied();
        ir.states[state_id as usize].default_child = resolved;
    }

    Ok(())
}

/// Decide whether `on(trigger) => target;` is a transition (target is a
/// declared state) or an action (target is a handler fn). Also enforce the
/// rules that only apply to one form or the other.
fn classify_target(
    ir: &mut Ir,
    target: &Ident,
    trig: &TriggerIr,
    state_names: &std::collections::HashSet<String>,
) -> syn::Result<HandlerKindIr> {
    let tname = target.to_string();
    if state_names.contains(&tname) {
        // Transition. A repeating timer transitioning is nonsensical (the
        // state exits on first fire and cancels the timer). Payload
        // bindings can't feed a target state — they go to action args.
        if let TriggerIr::Duration(tid) = trig {
            if ir.duration_triggers[*tid as usize].repeat {
                return Err(syn::Error::new(
                    target.span(),
                    "`every` timers are only valid on action handlers; \
                     transitioning to a state cancels the timer on first fire",
                ));
            }
        }
        if let TriggerIr::Event(_, variant, Some(_)) = trig {
            return Err(syn::Error::new(
                variant.span(),
                "event payload bindings are only valid when dispatching to an \
                 action handler; a transition target takes no arguments",
            ));
        }
        // State-id resolution happens later in `resolve_transitions`.
        Ok(HandlerKindIr::Transition(target.clone(), None))
    } else {
        // Action handler.
        let aid = match trig {
            TriggerIr::Event(_, variant, Some(payload)) => {
                intern_action_with_params(ir, target, &payload.fields, variant, payload.kind)?
            }
            _ => intern_action_unit(ir, target)?,
        };
        Ok(HandlerKindIr::Action(aid, target.clone()))
    }
}

/// Resolve transition targets from state idents to state ids after the tree is built.
pub fn resolve_transitions(ir: &mut Ir) -> syn::Result<()> {
    let mut name_map = std::collections::HashMap::new();
    for s in &ir.states {
        if s.name != "__Root" {
            name_map.insert(s.name.to_string(), s.id);
        }
    }

    let n = ir.states.len();
    for i in 0..n {
        let len = ir.states[i].handlers.len();
        for h in 0..len {
            if let HandlerKindIr::Transition(target, slot) = &ir.states[i].handlers[h].kind {
                if slot.is_some() {
                    continue;
                }
                let tname = target.to_string();
                let resolved = name_map.get(&tname).copied();
                let target_clone = target.clone();
                match resolved {
                    Some(id) => {
                        ir.states[i].handlers[h].kind =
                            HandlerKindIr::Transition(target_clone, Some(id));
                    }
                    None => {
                        return Err(syn::Error::new(
                            target.span(),
                            format!("unknown state `{}` in transition", target),
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}
