//! Compile-time validation (§6.2 / §9.9).

use std::collections::HashMap;

use syn::Ident;

use crate::ir::{HandlerKindIr, Ir, TriggerIr};
use crate::parse::{StateBody, StatechartInput};

pub fn validate(ir: &Ir) -> syn::Result<()> {
    validate_default_graph(ir)?;
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

        // `default` is optional: a composite without a `default(...)` is a
        // valid resting state — transitions targeting it land on the composite
        // itself, with no descent. Substates of such a composite are reachable
        // only via explicit transitions. (`default` must still be a direct
        // child when declared; that's checked in `validate_parse_tree`.)

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

/// Walk the default-edge graph and reject cycles. Each state has at most one
/// outgoing default edge, so the graph has out-degree ≤ 1 and any cycle is a
/// simple chain that loops. Detection: 3-color DFS via an iterative chain walk.
///
/// Cycles are rejected at compile time because at runtime, a default chain
/// fires as a sequence of immediate transitions on entry — a cycle would loop
/// forever, never returning control to the dispatcher.
fn validate_default_graph(ir: &Ir) -> syn::Result<()> {
    let n = ir.states.len();
    // 0 = white (unvisited), 1 = gray (on current chain), 2 = black (done).
    let mut color = vec![0u8; n];
    for start in 0..n {
        if color[start] != 0 {
            continue;
        }
        let mut chain: Vec<usize> = Vec::new();
        let mut cur = start;
        loop {
            if color[cur] == 2 {
                // Joined a previously-cleared subgraph. Chain so far is fine.
                break;
            }
            if color[cur] == 1 {
                // Back-edge into the current chain — cycle.
                let cycle_start = chain
                    .iter()
                    .position(|&x| x == cur)
                    .expect("gray node must be on current chain");
                let cycle: Vec<String> = chain[cycle_start..]
                    .iter()
                    .map(|&id| ir.states[id].name.to_string())
                    .collect();
                let pretty = format!("{} -> {}", cycle.join(" -> "), cycle[0]);
                let err_span = ir.states[chain[cycle_start]]
                    .default_target_ident
                    .as_ref()
                    .map(|(_, s)| *s)
                    .unwrap_or(ir.states[chain[cycle_start]].span);
                return Err(syn::Error::new(
                    err_span,
                    format!(
                        "default-transition cycle detected: {} \
                         (entering any state in this cycle would loop forever \
                          before reaching user code)",
                        pretty
                    ),
                ));
            }
            color[cur] = 1;
            chain.push(cur);
            match ir.states[cur].default_child {
                Some(next) => cur = next as usize,
                None => break,
            }
        }
        for &id in &chain {
            color[id] = 2;
        }
    }
    Ok(())
}

/// Additional checks that require the original parse tree (for counting duplicate
/// `default`/`terminate` with their spans, and the "terminate in non-root" error).
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
    // default(...) is a transition that fires immediately after the
    // declaring state's entries finish. It may be declared on any state
    // (composite or leaf) and may target any state in the chart. A leaf
    // with `default(T)` just transitions to T on every entry. The
    // default-graph cycle check in `validate_default_graph` (run from the
    // top-level `validate_parse_tree` over the lowered IR) ensures we
    // cannot construct an infinite chain at startup.
    for c in &body.children {
        let cname = c.name.to_string();
        validate_body(&c.body, false, &cname)?;
    }
    Ok(())
}
