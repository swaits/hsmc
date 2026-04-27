//! Compile-time validation (§6.2 / §9.9).

use std::collections::HashMap;

use syn::Ident;

use crate::ir::{HandlerKindIr, Ir, TriggerIr};
use crate::parse::{StateBody, StatechartInput};

pub fn validate(ir: &Ir) -> syn::Result<()> {
    // Duplicate state names
    let mut seen = HashMap::new();
    for s in &ir.states {
        if s.name == "__Root" {
            continue;
        }
        if let Some(_prev) = seen.insert(s.name.to_string(), s.id) {
            return Err(syn::Error::new(
                s.span,
                format!("duplicate state name `{}`", s.name),
            ));
        }
    }

    // Resolve transitions: done already? We need post-build resolution.
    // We'll do it here because we need IR mutable; but we're given &Ir.
    // Instead perform target lookup inline.
    let mut name_map: HashMap<String, u16> = ir
        .states
        .iter()
        .filter(|s| s.name != "__Root")
        .map(|s| (s.name.to_string(), s.id))
        .collect();
    // Per spec: the root state is targetable as a transition destination
    // by the chart's own name.
    if let Some(root) = ir.states.iter().find(|s| s.parent.is_none()) {
        name_map.insert(ir.name.to_string(), root.id);
    }

    for s in &ir.states {
        // Empty state (not root). A state with a `during:` is not empty —
        // the during is a real behavior scoped to the state's lifetime.
        if s.parent.is_some()
            && s.entries.is_empty()
            && s.exits.is_empty()
            && s.handlers.is_empty()
            && s.children.is_empty()
            && s.durings.is_empty()
        {
            return Err(syn::Error::new(s.span, format!("empty state `{}`", s.name)));
        }

        // During-level checks: each `during:` declares a list of root-context
        // fields it borrows `&mut`. We catch the obvious overlap cases at
        // macro expansion time so the user gets a clear message rather than
        // a raw rustc borrow-checker error on the generated `select` call.
        //
        // 1. Within a single `during:`, field names must be unique.
        // 2. Across durings *on the same state*, field names must be disjoint.
        //
        // Cross-hierarchy overlaps (parent vs. descendant) are left to rustc
        // — the borrow checker reports them accurately on the `&mut ctx.field`
        // call site in codegen.
        for d in &s.durings {
            let mut seen_in_one = std::collections::HashSet::new();
            for f in &d.fields {
                if !seen_in_one.insert(f.to_string()) {
                    return Err(syn::Error::new(
                        f.span(),
                        format!(
                            "field `{}` listed twice in `during: {}(...)` — each \
                             field can only be borrowed once per during",
                            f, d.fn_name
                        ),
                    ));
                }
            }
        }
        let mut field_owner: std::collections::HashMap<String, Ident> =
            std::collections::HashMap::new();
        for d in &s.durings {
            for f in &d.fields {
                if let Some(prev) = field_owner.insert(f.to_string(), d.fn_name.clone()) {
                    return Err(syn::Error::new(
                        f.span(),
                        format!(
                            "field `{}` is borrowed by two concurrent durings on state `{}` \
                             (`{}` and `{}`) — move one of them to a disjoint field or \
                             combine the activities into a single during that uses `select`",
                            f, s.name, prev, d.fn_name
                        ),
                    ));
                }
            }
        }

        // Children require default; default must be a direct child; no default allowed if no children.
        if !s.children.is_empty() && s.default_child.is_none() {
            return Err(syn::Error::new(
                s.span,
                format!(
                    "state `{}` has children but no `default` declaration",
                    s.name
                ),
            ));
        }

        // duplicate transitions per trigger in this state, AND transition
        // target (if present) must be declared after every action handler
        // on the same trigger — so the grammar's left-to-right reading
        // matches execution order (actions first, then transition).
        let mut trig_txn_seen: HashMap<String, usize> = HashMap::new();
        let mut trig_last_action_idx: HashMap<String, usize> = HashMap::new();
        for h in &s.handlers {
            let key = match &h.trigger {
                TriggerIr::Event(_, i, _) => format!("ev:{}", i),
                TriggerIr::Duration(id) => format!("dur:{}", id),
            };
            let trig_display = match &h.trigger {
                TriggerIr::Event(_, i, _) => i.to_string(),
                TriggerIr::Duration(id) => ir.duration_triggers[*id as usize].key.clone(),
            };
            let trig_span = match &h.trigger {
                TriggerIr::Event(_, i, _) => i.span(),
                TriggerIr::Duration(id) => ir.duration_triggers[*id as usize].span,
            };
            match &h.kind {
                HandlerKindIr::Transition(target, _) => {
                    if let Some(_prev) = trig_txn_seen.insert(key.clone(), h.decl_index) {
                        return Err(syn::Error::new(
                            trig_span,
                            format!(
                                "duplicate transition on trigger `{}` in state `{}`",
                                trig_display, s.name
                            ),
                        ));
                    }
                    if !name_map.contains_key(&target.to_string()) {
                        return Err(syn::Error::new(
                            target.span(),
                            format!("unknown state `{}` in transition", target),
                        ));
                    }
                    // Any action handler on the same trigger must have
                    // already been seen (smaller decl_index).
                    if let Some(&last_action_idx) = trig_last_action_idx.get(&key) {
                        if last_action_idx > h.decl_index {
                            return Err(syn::Error::new(
                                target.span(),
                                format!(
                                    "transition to state `{}` on trigger `{}` must come after \
                                     every action handler on the same trigger; hsmc runs \
                                     all actions before the transition, so declaring the state \
                                     target earlier misleads the reader",
                                    target, trig_display
                                ),
                            ));
                        }
                    }
                }
                HandlerKindIr::Action(_, _) => {
                    // If a transition on this trigger was already declared,
                    // this action would silently run *before* it at runtime
                    // — same ordering lie. Reject.
                    if let Some(&txn_idx) = trig_txn_seen.get(&key) {
                        if txn_idx < h.decl_index {
                            return Err(syn::Error::new(
                                trig_span,
                                format!(
                                    "action handler on trigger `{}` must be declared before the \
                                     state transition target; hsmc runs all actions before the \
                                     transition, so placing actions after the target misleads \
                                     the reader",
                                    trig_display
                                ),
                            ));
                        }
                    }
                    trig_last_action_idx.insert(key, h.decl_index);
                }
            }
        }
    }
    Ok(())
}

/// Additional checks that require the original parse tree (for counting duplicate
/// `default`/`terminate` with their spans, and the "default in childless state" /
/// "default not a direct child" / "terminate in non-root" errors).
pub fn validate_parse_tree(input: &StatechartInput) -> syn::Result<()> {
    let root_name = input.name.to_string();
    validate_body(&input.body, true, &root_name)
}

fn validate_body(body: &StateBody, is_root: bool, state_name: &str) -> syn::Result<()> {
    if body.default_count > 1 {
        return Err(syn::Error::new(
            body.default_span
                .unwrap_or_else(proc_macro2::Span::call_site),
            format!("multiple `default` declarations in state `{}`", state_name),
        ));
    }
    if body.terminate_count > 1 {
        return Err(syn::Error::new(
            body.terminate
                .as_ref()
                .map(|(_, s)| *s)
                .unwrap_or_else(proc_macro2::Span::call_site),
            "multiple `terminate` declarations",
        ));
    }
    if !is_root {
        if let Some((_, span)) = &body.terminate {
            return Err(syn::Error::new(
                *span,
                "`terminate` is only valid at the root level",
            ));
        }
    }
    if body.default_child.is_some() && body.children.is_empty() {
        return Err(syn::Error::new(
            body.default_span.unwrap(),
            format!(
                "`default` in state `{}` which has no child states",
                state_name
            ),
        ));
    }
    if let (Some((dc, dspan)), false) = (&body.default_child, body.children.is_empty()) {
        // default must be a direct child
        let is_child = body.children.iter().any(|c| c.name == *dc);
        if !is_child {
            return Err(syn::Error::new(
                *dspan,
                format!(
                    "`default({})` in state `{}`: `{}` is not a direct child of `{}`",
                    dc, state_name, dc, state_name
                ),
            ));
        }
    }
    for c in &body.children {
        let cname = c.name.to_string();
        validate_body(&c.body, false, &cname)?;
    }
    Ok(())
}
