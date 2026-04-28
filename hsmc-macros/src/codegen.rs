//! Code generation for the `statechart!` macro.

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::Ident;

use crate::ir::{HandlerKindIr, Ir, TriggerIr, VariantKind};
use crate::parse::{During, PayloadKind};

/// FNV-1a 64-bit hash of a chart's structural fingerprint. Stable across
/// rebuilds of the same chart definition; differs whenever any structural
/// element (state name, action name, transition target, default child,
/// timer trigger, event variant) changes.
///
/// Used as `<Chart>::CHART_HASH` so a journal recorded for one chart can
/// be rejected at replay time when the chart definition has drifted.
fn compute_chart_hash(ir: &Ir) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h: u64 = FNV_OFFSET;
    let mut feed = |bytes: &[u8]| {
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        h ^= 0xff;
        h = h.wrapping_mul(FNV_PRIME);
    };
    feed(ir.name.to_string().as_bytes());
    for s in &ir.states {
        feed(&s.id.to_le_bytes());
        feed(s.name.to_string().as_bytes());
        feed(&[
            s.parent.unwrap_or(u16::MAX).to_le_bytes()[0],
            s.parent.unwrap_or(u16::MAX).to_le_bytes()[1],
        ]);
        feed(&s.depth.to_le_bytes());
        feed(&s.default_child.unwrap_or(u16::MAX).to_le_bytes());
        for &aid in &s.entries {
            feed(&aid.to_le_bytes());
        }
        feed(&[0xee]); // separator: entries → exits
        for &aid in &s.exits {
            feed(&aid.to_le_bytes());
        }
        feed(&[0xff]); // separator: exits → handlers
        for h_ir in &s.handlers {
            match &h_ir.trigger {
                TriggerIr::Event(id, ident, _) => {
                    feed(&[0x01]);
                    feed(&id.to_le_bytes());
                    feed(ident.to_string().as_bytes());
                }
                TriggerIr::Duration(id) => {
                    feed(&[0x02]);
                    feed(&id.to_le_bytes());
                }
            }
            match &h_ir.kind {
                HandlerKindIr::Transition(name, slot) => {
                    feed(&[0x10]);
                    feed(name.to_string().as_bytes());
                    feed(&slot.unwrap_or(u16::MAX).to_le_bytes());
                }
                HandlerKindIr::Action(aid, name) => {
                    feed(&[0x11]);
                    feed(&aid.to_le_bytes());
                    feed(name.to_string().as_bytes());
                }
            }
        }
        feed(&[0xfe]); // separator: handlers → timers
        for &t in &s.owned_timers {
            feed(&t.to_le_bytes());
        }
        feed(&[0xfd]); // separator: timers → durings
        feed(&(s.durings.len() as u16).to_le_bytes());
    }
    feed(&[0xfc]); // separator: states → actions
    for a in &ir.actions {
        feed(a.name.to_string().as_bytes());
        feed(&(a.params.len() as u16).to_le_bytes());
    }
    feed(&[0xfb]); // separator: actions → events
    for e in &ir.event_variants {
        feed(e.name.to_string().as_bytes());
    }
    h
}

/// For each user-declared leaf state that has one or more `during:` activities
/// active (either declared on it, on any ancestor, or on root), return its
/// state id and the flattened list of durings in leaf-first order. Durings
/// declared on the same state follow their declaration order.
///
/// Leaf states with no active durings are omitted — the run loop uses its
/// default (channel+timer) path for those.
fn active_durings_per_leaf(ir: &Ir) -> Vec<(u16, Vec<During>)> {
    let mut out = Vec::new();
    for s in &ir.states {
        if !s.children.is_empty() {
            continue;
        }
        if s.parent.is_none() {
            continue; // synthetic __Root
        }
        let mut chain: Vec<During> = Vec::new();
        let mut cur = Some(s.id);
        while let Some(id) = cur {
            for d in &ir.states[id as usize].durings {
                chain.push(d.clone());
            }
            cur = ir.states[id as usize].parent;
        }
        if !chain.is_empty() {
            out.push((s.id, chain));
        }
    }
    out
}

/// Emit `let dN = fn(&mut self.ctx.fN_0, &mut self.ctx.fN_1, ...);` for each
/// during in `chain`. Returns the list of binding identifiers in order so the
/// call site can reference them in the `select!`/`select_N` arguments.
fn emit_during_bindings(chain: &[During]) -> (Vec<Ident>, TokenStream) {
    let mut idents = Vec::with_capacity(chain.len());
    let mut stmts: Vec<TokenStream> = Vec::with_capacity(chain.len());
    for (i, d) in chain.iter().enumerate() {
        let bind = format_ident!("__hsmc_d{}", i);
        let fn_name = &d.fn_name;
        let field_refs = d.fields.iter().map(|f| quote! { &mut self.ctx.#f });
        stmts.push(quote! {
            let #bind = #fn_name(#(#field_refs),*);
        });
        idents.push(bind);
    }
    (idents, quote! { #(#stmts)* })
}

/// Emit the per-leaf match arms for the tokio run loop. Each arm constructs
/// the active during: futures (split-borrowing context fields), then races
/// them against the event channel and the next-timer sleep via
/// `tokio::select!`. The default arm — used for leaves with no durings — is
/// emitted by the caller.
fn emit_tokio_race_arms_with_channel(ir: &Ir) -> Vec<TokenStream> {
    let mut arms = Vec::new();
    for (sid, chain) in active_durings_per_leaf(ir) {
        let (binds, stmts) = emit_during_bindings(&chain);
        let select_arms = binds.iter().map(|b| {
            quote! { ev = #b => __HsmcRace::Event(ev), }
        });
        arms.push(quote! {
            Some(#sid) => {
                #stmts
                let sleep = ::tokio::time::sleep(sleep_dur);
                ::tokio::pin!(sleep);
                let rx = self.__tokio_rx.as_mut().expect("hsmc: tokio channel");
                ::tokio::select! {
                    biased;
                    #(#select_arms)*
                    maybe = rx.recv() => match maybe {
                        Some(ev) => __HsmcRace::Event(ev),
                        None => __HsmcRace::ChannelClosed,
                    },
                    _ = &mut sleep => __HsmcRace::Timer,
                }
            }
        });
    }
    arms
}

/// Same as `emit_tokio_race_arms_with_channel` but for timer-only machines
/// (no `events:` declared, no channel). The default arm races only the timer.
fn emit_tokio_race_arms_timer_only(ir: &Ir) -> Vec<TokenStream> {
    let mut arms = Vec::new();
    for (sid, chain) in active_durings_per_leaf(ir) {
        let (binds, stmts) = emit_during_bindings(&chain);
        let select_arms = binds.iter().map(|b| {
            quote! { ev = #b => __HsmcRace::Event(ev), }
        });
        arms.push(quote! {
            Some(#sid) => {
                #stmts
                let sleep = ::tokio::time::sleep(sleep_dur);
                ::tokio::pin!(sleep);
                ::tokio::select! {
                    biased;
                    #(#select_arms)*
                    _ = &mut sleep => __HsmcRace::Timer,
                }
            }
        });
    }
    arms
}

/// Build a human-readable ASCII tree of the machine's hierarchy, suitable
/// for emitting as a `STATE_CHART` const. Each state shows its default-child
/// marker, entry/exit actions (if any), during activities, and handler
/// triggers. Used for `defmt::info!(M::STATE_CHART)` or panic messages.
fn emit_state_chart_string(ir: &Ir) -> String {
    let mut out = String::new();
    // Start with the machine name as the root label (root state name is
    // synthetic `__Root` — don't surface it).
    out.push_str(&ir.name.to_string());
    out.push('\n');
    // Top-level children of __Root: walk via __Root's children list.
    let root_children: Vec<u16> = ir.states[0].children.clone();
    let default_child_of_root = ir.states[0].default_child;
    for (i, cid) in root_children.iter().enumerate() {
        let last = i + 1 == root_children.len();
        let is_default = Some(*cid) == default_child_of_root;
        render_state(ir, *cid, "", last, is_default, &mut out);
    }
    out
}

fn render_state(
    ir: &Ir,
    sid: u16,
    prefix: &str,
    is_last: bool,
    is_default: bool,
    out: &mut String,
) {
    let s = &ir.states[sid as usize];
    let branch = if is_last { "└── " } else { "├── " };
    out.push_str(prefix);
    out.push_str(branch);
    if is_default {
        out.push_str("[default] ");
    }
    out.push_str(&s.name.to_string());
    out.push('\n');
    let child_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
    // Entry actions
    for aid in &s.entries {
        let name = &ir.actions[*aid as usize].name;
        out.push_str(&child_prefix);
        out.push_str("• entry: ");
        out.push_str(&name.to_string());
        out.push('\n');
    }
    // Exit actions
    for aid in &s.exits {
        let name = &ir.actions[*aid as usize].name;
        out.push_str(&child_prefix);
        out.push_str("• exit: ");
        out.push_str(&name.to_string());
        out.push('\n');
    }
    // Durings
    for d in &s.durings {
        out.push_str(&child_prefix);
        out.push_str("• during: ");
        out.push_str(&d.fn_name.to_string());
        out.push('(');
        for (i, f) in d.fields.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push_str(&f.to_string());
        }
        out.push_str(")\n");
    }
    // Handlers (summary)
    for h in &s.handlers {
        let trig = match &h.trigger {
            crate::ir::TriggerIr::Event(_, name, _) => format!("on({})", name),
            crate::ir::TriggerIr::Duration(tid) => {
                let d = &ir.duration_triggers[*tid as usize];
                let verb = if d.repeat { "every" } else { "after" };
                format!("on({} {})", verb, d.key.split(':').next().unwrap_or("?"))
            }
        };
        let target = match &h.kind {
            crate::ir::HandlerKindIr::Transition(tgt, _) => format!("→ {}", tgt),
            crate::ir::HandlerKindIr::Action(_, name) => format!("⇒ {}", name),
        };
        out.push_str(&child_prefix);
        out.push_str("• ");
        out.push_str(&trig);
        out.push(' ');
        out.push_str(&target);
        out.push('\n');
    }
    // Child states
    let default_child_of_s = s.default_child;
    for (i, cid) in s.children.iter().enumerate() {
        let last = i + 1 == s.children.len();
        let is_def = Some(*cid) == default_child_of_s;
        render_state(ir, *cid, &child_prefix, last, is_def, out);
    }
}

/// Return (select_fn_ident, either_variant_idents) for embassy_futures::select
/// with the given total arity. `arity` must be one of 2, 3, 4, 5, 6.
fn embassy_select_for_arity(arity: usize) -> (Ident, Vec<Ident>) {
    match arity {
        2 => (
            format_ident!("select"),
            vec![format_ident!("First"), format_ident!("Second")],
        ),
        3 => (
            format_ident!("select3"),
            vec![
                format_ident!("First"),
                format_ident!("Second"),
                format_ident!("Third"),
            ],
        ),
        4 => (
            format_ident!("select4"),
            vec![
                format_ident!("First"),
                format_ident!("Second"),
                format_ident!("Third"),
                format_ident!("Fourth"),
            ],
        ),
        5 => (
            format_ident!("select5"),
            vec![
                format_ident!("First"),
                format_ident!("Second"),
                format_ident!("Third"),
                format_ident!("Fourth"),
                format_ident!("Fifth"),
            ],
        ),
        6 => (
            format_ident!("select6"),
            vec![
                format_ident!("First"),
                format_ident!("Second"),
                format_ident!("Third"),
                format_ident!("Fourth"),
                format_ident!("Fifth"),
                format_ident!("Sixth"),
            ],
        ),
        _ => unreachable!("embassy_select_for_arity called with unsupported arity"),
    }
}

/// Emit embassy run-loop match arms for machines with a bound channel. Arity
/// per state = active_durings + 2 (channel + timer). Embassy caps at `select6`
/// → max 4 concurrent durings on a path with events. Over-limit paths emit
/// a `compile_error!` inside the embassy-only arm so tokio codegen still
/// compiles unchanged.
fn emit_embassy_race_arms_with_channel(ir: &Ir) -> Vec<TokenStream> {
    let mut arms = Vec::new();
    for (sid, chain) in active_durings_per_leaf(ir) {
        let n = chain.len();
        if n + 2 > 6 {
            let msg = format!(
                "too many concurrent `during:` activities on the active path for state id {} \
                 under the embassy feature ({} + channel + timer > 6); embassy_futures supports \
                 at most 4 concurrent durings on a path with events. Combine activities into a \
                 single during that uses `select` internally.",
                sid, n
            );
            arms.push(quote! {
                Some(#sid) => { ::core::compile_error!(#msg); unreachable!() }
            });
            continue;
        }
        let (binds, stmts) = emit_during_bindings(&chain);
        let (sel_fn, variants) = embassy_select_for_arity(n + 2);
        let ch_variant = &variants[n];
        let tm_variant = &variants[n + 1];
        let call_args = binds
            .iter()
            .map(|b| quote! { #b })
            .chain(std::iter::once(quote! { rx.receive() }))
            .chain(std::iter::once(quote! { timer_fut }));
        let either_path = if n + 2 == 2 {
            quote! { ::embassy_futures::select::Either }
        } else {
            let either_ty = format_ident!("Either{}", (n + 2).to_string());
            quote! { ::embassy_futures::select::#either_ty }
        };
        let during_match = binds.iter().enumerate().map(|(i, _b)| {
            let v = &variants[i];
            quote! { #either_path::#v(ev) => __HsmcRace::Event(ev), }
        });
        arms.push(quote! {
            Some(#sid) => {
                #stmts
                let timer_fut = async {
                    match next {
                        Some(d) => {
                            ::embassy_time::Timer::after(
                                ::embassy_time::Duration::from_micros(d.as_micros() as u64)
                            ).await;
                        }
                        None => ::core::future::pending::<()>().await,
                    }
                };
                let rx = self.__embassy_rx.as_ref().expect("hsmc: embassy channel");
                match ::embassy_futures::select::#sel_fn(#(#call_args),*).await {
                    #(#during_match)*
                    #either_path::#ch_variant(ev) => __HsmcRace::Event(ev),
                    #either_path::#tm_variant(()) => __HsmcRace::Timer,
                }
            }
        });
    }
    arms
}

/// Emit embassy run-loop match arms for timer-only machines (no channel).
/// Arity per state = active_durings + 1 (timer only). Max 5 concurrent
/// durings on a timer-only path.
fn emit_embassy_race_arms_timer_only(ir: &Ir) -> Vec<TokenStream> {
    let mut arms = Vec::new();
    for (sid, chain) in active_durings_per_leaf(ir) {
        let n = chain.len();
        if n == 0 {
            continue; // falls through to default (just timer)
        }
        if n + 1 > 6 {
            let msg = format!(
                "too many concurrent `during:` activities on the active path for state id {} \
                 under the embassy feature ({} + timer > 6); embassy_futures supports at most 5 \
                 concurrent durings on a timer-only path.",
                sid, n
            );
            arms.push(quote! {
                Some(#sid) => { ::core::compile_error!(#msg); unreachable!() }
            });
            continue;
        }
        let (binds, stmts) = emit_during_bindings(&chain);
        let (sel_fn, variants) = embassy_select_for_arity(n + 1);
        let call_args = binds
            .iter()
            .map(|b| quote! { #b })
            .chain(std::iter::once(quote! { timer_fut }));
        let either_path = if n + 1 == 2 {
            quote! { ::embassy_futures::select::Either }
        } else {
            let either_ty = format_ident!("Either{}", (n + 1).to_string());
            quote! { ::embassy_futures::select::#either_ty }
        };
        let tm_variant = &variants[n];
        let during_match = binds.iter().enumerate().map(|(i, _b)| {
            let v = &variants[i];
            quote! { #either_path::#v(ev) => __HsmcRace::Event(ev), }
        });
        arms.push(quote! {
            Some(#sid) => {
                #stmts
                let timer_fut = async {
                    match next {
                        Some(d) => {
                            ::embassy_time::Timer::after(
                                ::embassy_time::Duration::from_micros(d.as_micros() as u64)
                            ).await;
                        }
                        None => ::core::future::pending::<()>().await,
                    }
                };
                match ::embassy_futures::select::#sel_fn(#(#call_args),*).await {
                    #(#during_match)*
                    #either_path::#tm_variant(()) => __HsmcRace::Timer,
                }
            }
        });
    }
    arms
}

pub fn generate(ir: &Ir) -> TokenStream {
    let machine_name = &ir.name;
    let state_enum_name = format_ident!("{}State", machine_name);
    let actions_trait_name = format_ident!("{}Actions", machine_name);
    let action_ctx_name = format_ident!("{}ActionContext", machine_name);
    let ctx_alias_name = format_ident!("{}Ctx", machine_name);
    let sender_name = format_ident!("{}Sender", machine_name);
    let ctx_ty = &ir.context_ty;
    let ev_ty = &ir.event_ty;

    let n_states = ir.states.len();
    let n_actions = ir.actions.len();
    let n_dur_triggers = ir.duration_triggers.len();
    let max_timers = n_dur_triggers.max(1);

    let chart_hash = compute_chart_hash(ir);
    let chart_hash_lit = quote! { #chart_hash };

    // Public state enum — only user-declared states (skip __Root).
    let state_variants: Vec<_> = ir
        .states
        .iter()
        .filter(|s| s.parent.is_some())
        .map(|s| s.name.clone())
        .collect();

    let entries_of_arms: Vec<_> = ir
        .states
        .iter()
        .map(|s| {
            let sid = s.id;
            let entries_tok: Vec<_> = s.entries.iter().map(|a| quote! { #a }).collect();
            quote! { #sid => &[#(#entries_tok),*], }
        })
        .collect();
    let exits_of_arms: Vec<_> = ir
        .states
        .iter()
        .map(|s| {
            let sid = s.id;
            let exits_tok: Vec<_> = s.exits.iter().map(|a| quote! { #a }).collect();
            quote! { #sid => &[#(#exits_tok),*], }
        })
        .collect();

    // ── Per-id name lookup tables ─────────────────────────────────────
    //
    // Generated unconditionally at chart scope so any code in the
    // emitted module (machine impl, action context, etc.) can resolve
    // a u16 id to a `&'static str`. The tables are dead-code-eliminated
    // when no observation feature is on: with neither `journal` nor any
    // `trace-*` feature enabled, the `__chart_observe!` macro arms are
    // empty, so the call sites never reference these helpers and the
    // linker drops them. Marked `dead_code` to silence rustc when off.
    let chart_name_lit = ir.name.to_string();

    let state_name_arms: Vec<_> = ir
        .states
        .iter()
        .map(|s| {
            let sid = s.id;
            let nm = if s.parent.is_none() {
                // Synthesized root: render as the chart name (per spec
                // the root state IS the chart).
                ir.name.to_string()
            } else {
                s.name.to_string()
            };
            quote! { #sid => #nm, }
        })
        .collect();
    let action_name_arms: Vec<_> = ir
        .actions
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let aid = i as u16;
            let nm = a.name.to_string();
            quote! { #aid => #nm, }
        })
        .collect();
    let event_name_arms: Vec<_> = ir
        .event_variants
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let eid = i as u16;
            let nm = v.name.to_string();
            quote! { #eid => #nm, }
        })
        .collect();
    let timer_name_arms: Vec<_> = ir
        .duration_triggers
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let tid = i as u16;
            // The intern key looks like "<state>::<expr>:<repeat>". For
            // human readability render the expr text — fall back to
            // numeric id if the key parses oddly.
            let label = format!("t{}", tid);
            // Try to extract the expression body so the trace shows
            // something like `Duration::from_secs(5)`. Using `key` as
            // a stable label is OK for now; the expr token stream isn't
            // round-tripped through `to_string()` cleanly across rustc
            // versions, so prefer a synthesized short name.
            let _ = &d.key; // mark used for non-warn
            quote! { #tid => #label, }
        })
        .collect();
    // Per-(state, during-index) name lookup. The during's fn-name is
    // the closest thing to a stable identifier the user wrote.
    let during_name_arms: Vec<_> = {
        let mut arms = Vec::new();
        for s in &ir.states {
            let sid = s.id;
            for (di, d) in s.durings.iter().enumerate() {
                let did = di as u16;
                let nm = d.fn_name.to_string();
                arms.push(quote! { (#sid, #did) => #nm, });
            }
        }
        arms
    };
    // Pre-rendered transition reason strings for trace output.
    // `event:<VariantName>` for event-driven transitions.
    let event_reason_str_arms: Vec<_> = ir
        .event_variants
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let eid = i as u16;
            let s = format!("event:{}", v.name);
            quote! { #eid => #s, }
        })
        .collect();
    // `timer:<state>/<timer>` for timer-driven transitions. We
    // enumerate (state, timer) pairs that actually occur.
    let timer_reason_str_arms: Vec<_> = {
        let mut arms = Vec::new();
        for s in &ir.states {
            for &tid in &s.owned_timers {
                let sid = s.id;
                let sname = if s.parent.is_none() {
                    ir.name.to_string()
                } else {
                    s.name.to_string()
                };
                let tlabel = format!("t{}", tid);
                let s_str = format!("timer:{}/{}", sname, tlabel);
                arms.push(quote! { (#sid, #tid) => #s_str, });
            }
        }
        arms
    };

    let name_helpers = quote! {
        #[allow(dead_code)]
        #[doc(hidden)]
        #[inline]
        fn __state_name(sid: u16) -> &'static str {
            match sid {
                #(#state_name_arms)*
                _ => "<unknown>",
            }
        }
        #[allow(dead_code)]
        #[doc(hidden)]
        #[inline]
        fn __action_name(aid: u16) -> &'static str {
            match aid {
                #(#action_name_arms)*
                _ => "<unknown>",
            }
        }
        #[allow(dead_code, unreachable_patterns)]
        #[doc(hidden)]
        #[inline]
        fn __event_name(eid: u16) -> &'static str {
            match eid {
                #(#event_name_arms)*
                _ => "<unknown>",
            }
        }
        #[allow(dead_code, unreachable_patterns)]
        #[doc(hidden)]
        #[inline]
        fn __timer_name(tid: u16) -> &'static str {
            match tid {
                #(#timer_name_arms)*
                _ => "<unknown>",
            }
        }
        #[allow(dead_code, unreachable_patterns, unused_variables)]
        #[doc(hidden)]
        #[inline]
        fn __during_name(sid: u16, did: u16) -> &'static str {
            match (sid, did) {
                #(#during_name_arms)*
                _ => "<unknown>",
            }
        }
        #[allow(dead_code, unreachable_patterns)]
        #[doc(hidden)]
        #[inline]
        fn __event_reason_str(eid: u16) -> &'static str {
            match eid {
                #(#event_reason_str_arms)*
                _ => "event:?",
            }
        }
        #[allow(dead_code, unreachable_patterns, unused_variables)]
        #[doc(hidden)]
        #[inline]
        fn __timer_reason_str(sid: u16, tid: u16) -> &'static str {
            match (sid, tid) {
                #(#timer_reason_str_arms)*
                _ => "timer:?",
            }
        }
        #[allow(dead_code)]
        #[doc(hidden)]
        #[inline]
        fn __from_name(opt: Option<u16>) -> &'static str {
            match opt {
                Some(id) => __state_name(id),
                None => "<none>",
            }
        }
    };

    // ── Observation hook snippets ─────────────────────────────────────
    //
    // Each snippet emits ONE `__chart_observe!` invocation. The macro
    // dispatches per-variant to whichever sinks (journal + trace-*) are
    // enabled at compile time. Trace and journal share the same call
    // sites by construction — they cannot diverge.
    //
    // Names (state/action/event/timer/during) are passed as runtime
    // lookups against the small static tables generated above. With no
    // sink feature on, the macro arm body is empty and the lookup is
    // never expanded — zero overhead at the call site.

    let observe_started = quote! {
        ::hsmc::__chart_observe!(
            Started,
            &mut self.__journal,
            #chart_name_lit,
            Self::CHART_HASH
        );
    };
    let observe_enter_began = quote! {
        ::hsmc::__chart_observe!(
            EnterBegan,
            &mut self.__journal,
            #chart_name_lit,
            sid,
            __state_name(sid)
        );
    };
    let observe_entered = quote! {
        ::hsmc::__chart_observe!(
            Entered,
            &mut self.__journal,
            #chart_name_lit,
            sid,
            __state_name(sid)
        );
    };
    let observe_exit_began = quote! {
        ::hsmc::__chart_observe!(
            ExitBegan,
            &mut self.__journal,
            #chart_name_lit,
            sid,
            __state_name(sid)
        );
    };
    let observe_exited = quote! {
        ::hsmc::__chart_observe!(
            Exited,
            &mut self.__journal,
            #chart_name_lit,
            sid,
            __state_name(sid)
        );
    };
    let observe_action_entry = quote! {
        ::hsmc::__chart_observe!(
            ActionInvoked,
            &mut self.__journal,
            #chart_name_lit,
            sid, __state_name(sid),
            aid, __action_name(aid),
            Entry
        );
    };
    let observe_action_exit = quote! {
        ::hsmc::__chart_observe!(
            ActionInvoked,
            &mut self.__journal,
            #chart_name_lit,
            sid, __state_name(sid),
            aid, __action_name(aid),
            Exit
        );
    };
    let observe_action_handler = quote! {
        ::hsmc::__chart_observe!(
            ActionInvoked,
            &mut self.__journal,
            #chart_name_lit,
            __handler_state, __state_name(__handler_state),
            aid, __action_name(aid),
            Handler
        );
    };
    // TransitionFired — three call-site flavours. Codegen splices in
    // the appropriate one based on what triggered the transition.
    let observe_transition_fired_event = quote! {
        ::hsmc::__chart_observe!(
            TransitionFired,
            &mut self.__journal,
            #chart_name_lit,
            self.current, __from_name(self.current),
            target, __state_name(target),
            ::hsmc::TransitionReason::Event { event: __ev_id },
            __event_reason_str(__ev_id)
        );
    };
    let observe_transition_fired_timer = quote! {
        ::hsmc::__chart_observe!(
            TransitionFired,
            &mut self.__journal,
            #chart_name_lit,
            self.current, __from_name(self.current),
            target, __state_name(target),
            ::hsmc::TransitionReason::Timer {
                state: __t_state,
                timer: __t_trigger,
            },
            __timer_reason_str(__t_state, __t_trigger)
        );
    };
    #[allow(dead_code)]
    let _observe_transition_fired_internal = quote! {
        ::hsmc::__chart_observe!(
            TransitionFired,
            &mut self.__journal,
            #chart_name_lit,
            self.current, __from_name(self.current),
            target, __state_name(target),
            ::hsmc::TransitionReason::Internal,
            "internal"
        );
    };
    let observe_transition_complete = quote! {
        ::hsmc::__chart_observe!(
            TransitionComplete,
            &mut self.__journal,
            #chart_name_lit,
            __t_complete_from, __from_name(__t_complete_from),
            target, __state_name(target)
        );
    };
    let observe_event_received = quote! {
        ::hsmc::__chart_observe!(
            EventReceived,
            &mut self.__journal,
            #chart_name_lit,
            __ev_id,
            __event_name(__ev_id)
        );
    };
    let observe_event_delivered = quote! {
        ::hsmc::__chart_observe!(
            EventDelivered,
            &mut self.__journal,
            #chart_name_lit,
            __ev_id, __event_name(__ev_id),
            __handler_state, __state_name(__handler_state)
        );
    };
    let observe_event_dropped = quote! {
        ::hsmc::__chart_observe!(
            EventDropped,
            &mut self.__journal,
            #chart_name_lit,
            __ev_id, __event_name(__ev_id)
        );
    };
    let observe_timer_armed = quote! {
        ::hsmc::__chart_observe!(
            TimerArmed,
            &mut self.__journal,
            #chart_name_lit,
            sid, __state_name(sid),
            tid, __timer_name(tid),
            __duration_for(tid).as_nanos() as u64
        );
    };
    let observe_timer_cancelled = quote! {
        ::hsmc::__chart_observe!(
            TimerCancelled,
            &mut self.__journal,
            #chart_name_lit,
            sid, __state_name(sid),
            tid, __timer_name(tid)
        );
    };
    let observe_timer_fired = quote! {
        ::hsmc::__chart_observe!(
            TimerFired,
            &mut self.__journal,
            #chart_name_lit,
            __t_state, __state_name(__t_state),
            __t_trigger, __timer_name(__t_trigger)
        );
    };
    let observe_terminated = quote! {
        ::hsmc::__chart_observe!(
            Terminated,
            &mut self.__journal,
            #chart_name_lit
        );
    };
    let observe_terminate_requested = quote! {
        ::hsmc::__chart_observe!(
            TerminateRequested,
            &mut self.__journal,
            #chart_name_lit,
            __ev_id, __event_name(__ev_id)
        );
    };

    // Per-state during count, used to emit DuringStarted/Cancelled
    // events one per declared during in the state.
    let durings_count_arms: Vec<TokenStream> = ir
        .states
        .iter()
        .map(|s| {
            let sid = s.id;
            let n = s.durings.len() as u16;
            quote! { #sid => #n, }
        })
        .collect();
    let observe_durings_started = quote! {
        let __dn = Self::__durings_count(sid);
        let mut __di: u16 = 0;
        while __di < __dn {
            ::hsmc::__chart_observe!(
                DuringStarted,
                &mut self.__journal,
                #chart_name_lit,
                sid, __state_name(sid),
                __di, __during_name(sid, __di)
            );
            __di += 1;
        }
    };
    let observe_durings_cancelled = quote! {
        let __dn = Self::__durings_count(sid);
        let mut __di: u16 = 0;
        while __di < __dn {
            ::hsmc::__chart_observe!(
                DuringCancelled,
                &mut self.__journal,
                #chart_name_lit,
                sid, __state_name(sid),
                __di, __during_name(sid, __di)
            );
            __di += 1;
        }
    };

    let parent_table = ir
        .states
        .iter()
        .map(|s| match s.parent {
            Some(p) => quote! { Some(#p) },
            None => quote! { None },
        })
        .collect::<Vec<_>>();
    let depth_table = ir
        .states
        .iter()
        .map(|s| {
            let d = s.depth;
            quote! { #d }
        })
        .collect::<Vec<_>>();
    let default_child_table = ir
        .states
        .iter()
        .map(|s| match s.default_child {
            Some(c) => quote! { Some(#c) },
            None => quote! { None },
        })
        .collect::<Vec<_>>();
    let state_id_to_variant_arms = ir
        .states
        .iter()
        .filter(|s| s.parent.is_some())
        .map(|s| {
            let id = s.id;
            let nm = &s.name;
            quote! { #id => #state_enum_name::#nm, }
        })
        .collect::<Vec<_>>();
    // Trait methods, possibly with typed params for payload-bearing actions.
    let action_methods_sync = ir
        .actions
        .iter()
        .map(|a| {
            let name = &a.name;
            let params = a.params.iter().map(|f| {
                let n = &f.name;
                let t = &f.ty;
                quote! { #n: #t }
            });
            quote! { fn #name(&mut self, #(#params),*); }
        })
        .collect::<Vec<_>>();
    let action_methods_async = ir
        .actions
        .iter()
        .map(|a| {
            let name = &a.name;
            let params = a.params.iter().map(|f| {
                let n = &f.name;
                let t = &f.ty;
                quote! { #n: #t }
            });
            quote! {
                fn #name(&mut self, #(#params),*) -> impl ::core::future::Future<Output = ()>;
            }
        })
        .collect::<Vec<_>>();

    // Dispatch arms: for unit-signature actions call straight through; for
    // payload-bearing actions, re-match on the passed event and destructure.
    // Payload dispatch binds each field by reference (scrutinee is `&Ev`)
    // then `.clone()`s it to produce an owned value matching the handler's
    // declared signature. For `Copy` types this optimizes away.
    let run_action_arms_sync = ir
        .actions
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let i = i as u16;
            let name = &a.name;
            if a.params.is_empty() {
                quote! { #i => <Ctx as #actions_trait_name>::#name(ctx), }
            } else {
                let field_idents: Vec<_> = a.params.iter().map(|f| f.name.clone()).collect();
                let clone_args = field_idents.iter().map(|n| quote! { #n.clone() });
                let variant_arms = a.bound_variants.iter().map(|bv| {
                    let vname = &bv.name;
                    let binds = &field_idents;
                    let call = {
                        let clones = clone_args.clone();
                        quote! { <Ctx as #actions_trait_name>::#name(ctx, #(#clones),*); }
                    };
                    match bv.kind {
                        PayloadKind::Tuple => quote! {
                            #ev_ty::#vname(#(#binds),*) => { #call }
                        },
                        PayloadKind::Struct => quote! {
                            #ev_ty::#vname { #(#binds),* } => { #call }
                        },
                    }
                });
                quote! {
                    #i => {
                        if let Some(__ev) = __event {
                            match __ev {
                                #(#variant_arms)*
                                _ => {}
                            }
                        }
                    }
                }
            }
        })
        .collect::<Vec<_>>();
    let run_action_arms_async = ir
        .actions
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let i = i as u16;
            let name = &a.name;
            if a.params.is_empty() {
                quote! { #i => <Ctx as #actions_trait_name>::#name(ctx).await, }
            } else {
                let field_idents: Vec<_> = a.params.iter().map(|f| f.name.clone()).collect();
                let clone_args = field_idents.iter().map(|n| quote! { #n.clone() });
                let variant_arms = a.bound_variants.iter().map(|bv| {
                    let vname = &bv.name;
                    let binds = &field_idents;
                    let call = {
                        let clones = clone_args.clone();
                        quote! { <Ctx as #actions_trait_name>::#name(ctx, #(#clones),*).await; }
                    };
                    match bv.kind {
                        PayloadKind::Tuple => quote! {
                            #ev_ty::#vname(#(#binds),*) => { #call }
                        },
                        PayloadKind::Struct => quote! {
                            #ev_ty::#vname { #(#binds),* } => { #call }
                        },
                    }
                });
                quote! {
                    #i => {
                        if let Some(__ev) = __event {
                            match __ev {
                                #(#variant_arms)*
                                _ => {}
                            }
                        }
                    }
                }
            }
        })
        .collect::<Vec<_>>();
    let duration_expr_arms = ir
        .duration_triggers
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let i = i as u16;
            let expr = &d.expr;
            quote! { #i => { let __d: ::hsmc::Duration = #expr; __d }, }
        })
        .collect::<Vec<_>>();
    let duration_repeat_arms = ir
        .duration_triggers
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let i = i as u16;
            let repeat = d.repeat;
            quote! { #i => #repeat, }
        })
        .collect::<Vec<_>>();
    let event_variant_arms = ir
        .event_variants
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let i = i as u16;
            let name = &v.name;
            let pat = match v.kind {
                VariantKind::Unit => quote! { #ev_ty::#name },
                VariantKind::Tuple => quote! { #ev_ty::#name(..) },
                VariantKind::Struct => quote! { #ev_ty::#name { .. } },
            };
            quote! { #pat => #i, }
        })
        .collect::<Vec<_>>();
    let state_handler_arms = ir
        .states
        .iter()
        .map(|s| {
            use std::collections::BTreeMap;
            #[derive(Default)]
            struct Group {
                action_ids: Vec<u16>,
                transition_target: Option<u16>,
            }
            let mut groups: BTreeMap<(u8, u16), Group> = BTreeMap::new();
            let mut handlers = s.handlers.iter().collect::<Vec<_>>();
            handlers.sort_by_key(|h| h.decl_index);
            for h in handlers {
                let key = match &h.trigger {
                    TriggerIr::Event(id, _, _) => (0u8, *id),
                    TriggerIr::Duration(id) => (1u8, *id),
                };
                let g = groups.entry(key).or_default();
                match &h.kind {
                    HandlerKindIr::Action(aid, _) => g.action_ids.push(*aid),
                    HandlerKindIr::Transition(_, Some(tid)) => g.transition_target = Some(*tid),
                    HandlerKindIr::Transition(_, None) => {}
                }
            }
            let sid = s.id;
            let inner_arms = groups
                .iter()
                .map(|((kind, tid), g)| {
                    let kind = *kind;
                    let tid = *tid;
                    let actions = g.action_ids.iter().map(|a| quote! { #a });
                    let transition_tok = match g.transition_target {
                        Some(t) => quote! { Some(#t) },
                        None => quote! { None },
                    };
                    quote! {
                        (#kind, #tid) => {
                            let actions: &'static [u16] = &[#(#actions),*];
                            let target: Option<u16> = #transition_tok;
                            return Some((actions, target));
                        }
                    }
                })
                .collect::<Vec<_>>();
            quote! {
                #sid => {
                    match (trigger_kind, trigger_id) {
                        #(#inner_arms)*
                        _ => {}
                    }
                }
            }
        })
        .collect::<Vec<_>>();
    let owned_timers_arms = ir
        .states
        .iter()
        .map(|s| {
            let sid = s.id;
            let ids = s.owned_timers.iter().map(|t| quote! { #t });
            quote! { #sid => &[#(#ids),*], }
        })
        .collect::<Vec<_>>();
    let terminate_match = if let Some(tev) = &ir.terminate_event {
        // Use the kind recorded for this variant (unit/tuple/struct).
        let kind = ir
            .event_variants
            .iter()
            .find(|v| v.name == *tev)
            .map(|v| v.kind)
            .unwrap_or(VariantKind::Unit);
        let pat = match kind {
            VariantKind::Unit => quote! { #ev_ty::#tev },
            VariantKind::Tuple => quote! { #ev_ty::#tev(..) },
            VariantKind::Struct => quote! { #ev_ty::#tev { .. } },
        };
        quote! {
            if matches!(__ev, #pat) { return TerminateCheck::Yes; }
        }
    } else {
        quote! {}
    };

    let _ = n_actions;

    // Doc strings as literals for quoting.
    let state_enum_doc = format!(
        "Enum of every state declared in the `{}` statechart. Each variant names a user-declared state; the root is implicit and never surfaced.",
        machine_name
    );
    let actions_trait_doc = format!(
        "Trait implemented by the user to provide action function bodies for the `{}` statechart. One method per unique action name.",
        machine_name
    );
    let action_ctx_doc = format!(
        "Wrapper passed to action methods of the `{}` statechart. `Deref`s to the user's context and exposes [`emit()`](#method.emit) for re-entrant event emission (§4.3).",
        machine_name
    );
    let ctx_alias_doc = format!(
        "Convenience type alias for [`{}ActionContext`] (spec §12.4).",
        machine_name
    );
    let machine_doc = format!(
        "The `{}` state machine. Drive it with [`step()`](Self::step) or, under an async feature, `run().await`. See the crate docs for the generated API (§4.4).",
        machine_name
    );
    let sender_doc = format!(
        "Handle for sending events into a running `{}` from other threads, tasks, or ISRs (§4.5).",
        machine_name
    );

    // Optional-events branch: decide whether to emit Sender / sender() / channel
    // plumbing at all. When events: was omitted the machine is timer-only and
    // has no Sender type.
    let emit_sender = !ir.events_omitted;

    // Synthesize the empty event type when events: was omitted.
    let no_events_enum = if ir.events_omitted {
        quote! {
            #[doc(hidden)]
            #[allow(dead_code)]
            pub enum __NoEvents {}
        }
    } else {
        quote! {}
    };

    // Race-loop match arms for each run() variant. When the active state has
    // `during:` activities on its path, the arm constructs those futures by
    // split-borrowing context fields and races them against the channel and
    // next-timer sleep via `tokio::select!` / `embassy_futures::select`.
    let tokio_race_arms_with_ch = emit_tokio_race_arms_with_channel(ir);
    let tokio_race_arms_timer_only = emit_tokio_race_arms_timer_only(ir);
    let embassy_race_arms_with_ch = emit_embassy_race_arms_with_channel(ir);
    let embassy_race_arms_timer_only = emit_embassy_race_arms_timer_only(ir);

    // Whether any state in the machine declares at least one `during:`. When
    // false, run() uses the original v0.1 pre-during code path verbatim,
    // which keeps backward compatibility bit-for-bit for machines that
    // never opted into the new feature.
    let has_any_durings = ir.states.iter().any(|s| !s.durings.is_empty());

    // The tokio with-channel run() body. v0.1 verbatim when no durings are
    // declared; otherwise a per-state match dispatching to `select!` with
    // the appropriate active during set.
    let tokio_run_with_ch_body = if has_any_durings {
        quote! {
            #[allow(dead_code)]
            enum __HsmcRace<E> { Event(E), ChannelClosed, Timer }
            if self.terminated { return Err(::hsmc::HsmcError::AlreadyTerminated); }
            let mut next = self.step(::hsmc::Duration::ZERO).await;
            if self.take_overflow() { return Err(::hsmc::HsmcError::QueueFull); }
            let mut last_instant = ::tokio::time::Instant::now();
            while !self.terminated {
                if self.take_overflow() { return Err(::hsmc::HsmcError::QueueFull); }
                if !self.queue.is_empty() {
                    let now = ::tokio::time::Instant::now();
                    let elapsed = now.duration_since(last_instant);
                    last_instant = now;
                    next = self.step(elapsed).await;
                    continue;
                }
                let sleep_dur = next.unwrap_or(::hsmc::Duration::from_secs(3600));
                let race: __HsmcRace<#ev_ty> = match self.current {
                    #(#tokio_race_arms_with_ch)*
                    _ => {
                        let sleep = ::tokio::time::sleep(sleep_dur);
                        ::tokio::pin!(sleep);
                        let rx = self.__tokio_rx.as_mut().expect("hsmc: tokio channel");
                        ::tokio::select! {
                            biased;
                            maybe = rx.recv() => match maybe {
                                Some(ev) => __HsmcRace::Event(ev),
                                None => __HsmcRace::ChannelClosed,
                            },
                            _ = &mut sleep => __HsmcRace::Timer,
                        }
                    }
                };
                match race {
                    __HsmcRace::Event(ev) => { let _ = self.queue.push_back(ev); }
                    __HsmcRace::ChannelClosed => { if next.is_none() { break; } }
                    __HsmcRace::Timer => {}
                }
                let now = ::tokio::time::Instant::now();
                let elapsed = now.duration_since(last_instant);
                last_instant = now;
                next = self.step(elapsed).await;
            }
            Ok(())
        }
    } else {
        quote! {
            if self.terminated { return Err(::hsmc::HsmcError::AlreadyTerminated); }
            let mut next = self.step(::hsmc::Duration::ZERO).await;
            if self.take_overflow() { return Err(::hsmc::HsmcError::QueueFull); }
            let mut last_instant = ::tokio::time::Instant::now();
            while !self.terminated {
                if self.take_overflow() { return Err(::hsmc::HsmcError::QueueFull); }
                if !self.queue.is_empty() {
                    let now = ::tokio::time::Instant::now();
                    let elapsed = now.duration_since(last_instant);
                    last_instant = now;
                    next = self.step(elapsed).await;
                    continue;
                }
                let sleep_dur = next.unwrap_or(::hsmc::Duration::from_secs(3600));
                let sleep = ::tokio::time::sleep(sleep_dur);
                ::tokio::pin!(sleep);
                let rx = self.__tokio_rx.as_mut().expect("tokio channel");
                ::tokio::select! {
                    maybe_ev = rx.recv() => {
                        match maybe_ev {
                            Some(ev) => { let _ = self.queue.push_back(ev); }
                            None => {
                                if next.is_none() { break; }
                            }
                        }
                        let now = ::tokio::time::Instant::now();
                        let elapsed = now.duration_since(last_instant);
                        last_instant = now;
                        next = self.step(elapsed).await;
                    }
                    _ = &mut sleep => {
                        let now = ::tokio::time::Instant::now();
                        let elapsed = now.duration_since(last_instant);
                        last_instant = now;
                        next = self.step(elapsed).await;
                    }
                }
            }
            Ok(())
        }
    };

    // The embassy with-channel run() body. v0.1 verbatim when no durings
    // are declared; otherwise a per-state match dispatching to
    // embassy_futures::select_N with the appropriate active during set.
    let embassy_run_with_ch_body = if has_any_durings {
        quote! {
            #[allow(dead_code)]
            enum __HsmcRace<E> { Event(E), ChannelClosed, Timer }
            if self.terminated { return Err(::hsmc::HsmcError::AlreadyTerminated); }
            let mut next = self.step(::hsmc::Duration::ZERO).await;
            if self.take_overflow() { return Err(::hsmc::HsmcError::QueueFull); }
            let mut last_instant = ::embassy_time::Instant::now();
            while !self.terminated {
                if self.take_overflow() { return Err(::hsmc::HsmcError::QueueFull); }
                if !self.queue.is_empty() {
                    let now = ::embassy_time::Instant::now();
                    let elapsed_us = now.duration_since(last_instant).as_micros();
                    last_instant = now;
                    next = self.step(::hsmc::Duration::from_micros(elapsed_us)).await;
                    continue;
                }
                let race: __HsmcRace<#ev_ty> = match self.current {
                    #(#embassy_race_arms_with_ch)*
                    _ => {
                        let timer_fut = async {
                            match next {
                                Some(d) => {
                                    ::embassy_time::Timer::after(
                                        ::embassy_time::Duration::from_micros(d.as_micros() as u64)
                                    ).await;
                                }
                                None => ::core::future::pending::<()>().await,
                            }
                        };
                        let rx = self.__embassy_rx.as_ref().expect("hsmc: embassy channel");
                        match ::embassy_futures::select::select(rx.receive(), timer_fut).await {
                            ::embassy_futures::select::Either::First(ev) => __HsmcRace::Event(ev),
                            ::embassy_futures::select::Either::Second(()) => __HsmcRace::Timer,
                        }
                    }
                };
                match race {
                    __HsmcRace::Event(ev) => { let _ = self.queue.push_back(ev); }
                    __HsmcRace::ChannelClosed => {}
                    __HsmcRace::Timer => {}
                }
                let now = ::embassy_time::Instant::now();
                let elapsed_us = now.duration_since(last_instant).as_micros();
                last_instant = now;
                next = self.step(::hsmc::Duration::from_micros(elapsed_us)).await;
            }
            Ok(())
        }
    } else {
        quote! {
            if self.terminated { return Err(::hsmc::HsmcError::AlreadyTerminated); }
            let rx = self.__embassy_rx
                .expect("hsmc: embassy channel (populated by Machine::new)");
            let mut next = self.step(::hsmc::Duration::ZERO).await;
            if self.take_overflow() { return Err(::hsmc::HsmcError::QueueFull); }
            let mut last_instant = ::embassy_time::Instant::now();
            while !self.terminated {
                if self.take_overflow() { return Err(::hsmc::HsmcError::QueueFull); }
                if !self.queue.is_empty() {
                    let now = ::embassy_time::Instant::now();
                    let elapsed_us = now.duration_since(last_instant).as_micros();
                    last_instant = now;
                    next = self.step(::hsmc::Duration::from_micros(elapsed_us)).await;
                    continue;
                }
                let timer_fut = async {
                    match next {
                        Some(d) => {
                            ::embassy_time::Timer::after(
                                ::embassy_time::Duration::from_micros(d.as_micros() as u64)
                            ).await;
                        }
                        None => ::core::future::pending::<()>().await,
                    }
                };
                match ::embassy_futures::select::select(rx.receive(), timer_fut).await {
                    ::embassy_futures::select::Either::First(ev) => {
                        let _ = self.queue.push_back(ev);
                    }
                    ::embassy_futures::select::Either::Second(()) => {}
                }
                let now = ::embassy_time::Instant::now();
                let elapsed_us = now.duration_since(last_instant).as_micros();
                last_instant = now;
                next = self.step(::hsmc::Duration::from_micros(elapsed_us)).await;
            }
            Ok(())
        }
    };

    // State ids whose active `during:` chain is non-empty. Used by timer-only
    // run() bodies to decide whether to exit when no timers are pending: if
    // the current state has an active during, the run loop should keep
    // awaiting rather than break (the during might yet produce an event).
    let states_with_active_durings: Vec<u16> = active_durings_per_leaf(ir)
        .into_iter()
        .map(|(sid, _)| sid)
        .collect();
    let has_active_during_arms: Vec<TokenStream> = states_with_active_durings
        .iter()
        .map(|sid| quote! { Some(#sid) => true, })
        .collect();

    // Human-readable ASCII diagram of the machine's hierarchy. Emitted as a
    // `pub const STATE_CHART: &str` on the machine struct. Useful for
    // `defmt::info!`, panic messages, and documentation.
    let state_chart_str = emit_state_chart_string(ir);

    // Sender / sender() under tokio. With eager channel creation in
    // `with_queue_capacity_internal()`, `sender()` is `&self`.
    let tokio_sender_impl = if emit_sender {
        quote! {
            #[cfg(feature = "tokio")]
            #[doc = #sender_doc]
            #[derive(Clone)]
            pub struct #sender_name {
                tx: ::tokio::sync::mpsc::UnboundedSender<#ev_ty>,
            }
            #[cfg(feature = "tokio")]
            impl #sender_name {
                /// Send an event to the machine. Returns `Err(AlreadyTerminated)` if
                /// the machine has terminated and the receiver has been dropped.
                pub fn send(&self, event: #ev_ty) -> ::core::result::Result<(), ::hsmc::HsmcError> {
                    self.tx.send(event).map_err(|_| ::hsmc::HsmcError::AlreadyTerminated)
                }
            }

            #[cfg(feature = "tokio")]
            impl<const __QN: usize> #machine_name<__QN> {
                /// Obtain a clonable, `Send` handle for pushing events into this
                /// machine. The channel is eagerly created in `new()` (§4.4).
                pub fn sender(&self) -> #sender_name {
                    #sender_name { tx: self.__tokio_tx.as_ref().expect("hsmc: tokio channel").clone() }
                }

                /// Run the machine to completion on the current Tokio runtime.
                pub async fn run(&mut self) -> ::core::result::Result<(), ::hsmc::HsmcError> {
                    #tokio_run_with_ch_body
                }
            }
        }
    } else {
        // Timer-only: no Sender, no tokio channel, but keep a timer-only run().
        quote! {
            #[cfg(feature = "tokio")]
            impl<const __QN: usize> #machine_name<__QN> {
                /// Run the machine to completion, honouring timers and
                /// `during:` activities. No event channel is generated
                /// because this statechart declares no `events:` (§12.2).
                pub async fn run(&mut self) -> ::core::result::Result<(), ::hsmc::HsmcError> {
                    #[allow(dead_code)]
                    enum __HsmcRace<E> { Event(E), ChannelClosed, Timer }
                    if self.terminated { return Err(::hsmc::HsmcError::AlreadyTerminated); }
                    let mut next = self.step(::hsmc::Duration::ZERO).await;
                    if self.take_overflow() { return Err(::hsmc::HsmcError::QueueFull); }
                    let mut last_instant = ::tokio::time::Instant::now();
                    while !self.terminated {
                        if self.take_overflow() { return Err(::hsmc::HsmcError::QueueFull); }
                        if !self.queue.is_empty() {
                            let now = ::tokio::time::Instant::now();
                            let elapsed = now.duration_since(last_instant);
                            last_instant = now;
                            next = self.step(elapsed).await;
                            continue;
                        }
                        let sleep_dur = match next {
                            Some(d) => d,
                            None => {
                                // No timers pending. If the current state
                                // has no `during:` activities either, there
                                // is nothing left to wait on — exit.
                                let has_any_during: bool = match self.current {
                                    #(#has_active_during_arms)*
                                    _ => false,
                                };
                                if !has_any_during { break; }
                                ::hsmc::Duration::from_secs(3600)
                            }
                        };
                        let race: __HsmcRace<#ev_ty> = match self.current {
                            #(#tokio_race_arms_timer_only)*
                            _ => {
                                ::tokio::time::sleep(sleep_dur).await;
                                __HsmcRace::Timer
                            }
                        };
                        match race {
                            __HsmcRace::Event(ev) => { let _ = self.queue.push_back(ev); }
                            __HsmcRace::ChannelClosed => {}
                            __HsmcRace::Timer => {}
                        }
                        let now = ::tokio::time::Instant::now();
                        let elapsed = now.duration_since(last_instant);
                        last_instant = now;
                        next = self.step(elapsed).await;
                    }
                    Ok(())
                }
            }
        }
    };

    // Default (no feature): stub sender that documents the limitation. We
    // intentionally keep it non-functional — a proper `Send + Clone` handle
    // without `alloc` or a platform primitive is out of scope for v0.1. See
    // the `tokio` or `embassy` features for a working `sender()`.
    let default_sender_impl = if emit_sender {
        quote! {
            #[cfg(not(any(feature = "tokio", feature = "embassy")))]
            #[doc = #sender_doc]
            ///
            /// **Limitation:** without the `tokio` or `embassy` feature, the
            /// default sender is a non-functional stub and
            /// [`send()`](Self::send) always returns
            /// [`HsmcError::AlreadyTerminated`]. Use the `tokio` or `embassy`
            /// feature to get a working cross-thread sender.
            #[derive(Clone)]
            pub struct #sender_name { _priv: () }
            #[cfg(not(any(feature = "tokio", feature = "embassy")))]
            impl #sender_name {
                /// Always returns [`HsmcError::AlreadyTerminated`] under
                /// default features — enable `tokio` or `embassy` for a
                /// functional implementation.
                pub fn send(&self, _event: #ev_ty) -> ::core::result::Result<(), ::hsmc::HsmcError> {
                    Err(::hsmc::HsmcError::AlreadyTerminated)
                }
            }

            #[cfg(not(any(feature = "tokio", feature = "embassy")))]
            impl<const __QN: usize> #machine_name<__QN> {
                /// Non-functional stub under default features — see
                /// [`#sender_name`] for the limitation.
                pub fn sender(&self) -> #sender_name { #sender_name { _priv: () } }
            }
        }
    } else {
        quote! {}
    };

    // Embassy: first-class support. `Machine::new(ctx, &'static CHAN)` stores
    // type-erased send/receive endpoints so the Machine struct stays free of
    // a `M: RawMutex` generic. `sender()` hands out a `Copy + Send + Sync`
    // newtype usable from ISRs (`try_send`) and tasks (`send().await`).
    let send_assert_name = format_ident!("__hsmc_embassy_ev_send_assert_{}", machine_name);
    let embassy_sender_impl = if emit_sender {
        quote! {
            // Compile-time assertion: the event type must be `Send` under the
            // embassy feature (its sender handle is `Send + Sync`, which
            // requires `E: Send`). Fails with a pointer to `#ev_ty`.
            #[cfg(feature = "embassy")]
            #[allow(dead_code, non_upper_case_globals)]
            const #send_assert_name: fn() = || {
                fn __assert_send<T: ::core::marker::Send>() {}
                __assert_send::<#ev_ty>();
            };

            #[cfg(feature = "embassy")]
            #[doc = #sender_doc]
            ///
            /// `Copy + Send + Sync`-able handle around a
            /// [`embassy_sync::channel::SendDynamicSender`]. Safe to capture in
            /// a `static` for ISR use and to clone across Embassy tasks.
            #[derive(Clone, Copy)]
            pub struct #sender_name {
                inner: ::embassy_sync::channel::SendDynamicSender<'static, #ev_ty>,
            }
            #[cfg(feature = "embassy")]
            impl #sender_name {
                /// Non-blocking send. Returns
                /// [`HsmcError::QueueFull`] if the channel has no room. Safe
                /// from ISR context when the channel is backed by
                /// `CriticalSectionRawMutex`.
                #[track_caller]
                pub fn try_send(self, event: #ev_ty) -> ::core::result::Result<(), ::hsmc::HsmcError> {
                    self.inner.try_send(event).map_err(|_| ::hsmc::HsmcError::QueueFull)
                }
                /// Async send. Backpressures (awaits) when the channel is
                /// full. Use from Embassy tasks that must not drop events.
                pub async fn send(self, event: #ev_ty) -> ::core::result::Result<(), ::hsmc::HsmcError> {
                    self.inner.send(event).await;
                    Ok(())
                }
                /// Build a sender directly from a user-declared static
                /// channel — convenient for ISR shims that run before the
                /// machine task has spawned. The channel's event type and
                /// capacity must match the machine.
                pub fn from_channel<__M, const __N: usize>(
                    channel: &'static ::embassy_sync::channel::Channel<__M, #ev_ty, __N>,
                ) -> Self
                where
                    __M: ::embassy_sync::blocking_mutex::raw::RawMutex
                        + ::core::marker::Sync
                        + ::core::marker::Send
                        + 'static,
                    #ev_ty: ::core::marker::Send,
                {
                    Self { inner: channel.sender().into() }
                }
            }

            #[cfg(feature = "embassy")]
            impl #machine_name<8> {
                /// Construct a new machine with the default event-queue
                /// capacity of 8, bound to a user-declared static
                /// `embassy_sync::channel::Channel`. The channel's capacity
                /// must match the machine's `__QN` — here, 8.
                pub fn new<__M>(
                    ctx: #ctx_ty,
                    channel: &'static ::embassy_sync::channel::Channel<__M, #ev_ty, 8>,
                ) -> Self
                where
                    __M: ::embassy_sync::blocking_mutex::raw::RawMutex
                        + ::core::marker::Sync
                        + ::core::marker::Send
                        + 'static,
                    #ev_ty: ::core::marker::Send,
                {
                    let mut __self = <#machine_name<8>>::with_queue_capacity_internal(ctx);
                    __self.__embassy_rx = Some(channel.receiver().into());
                    __self.__embassy_tx = Some(channel.sender().into());
                    __self
                }
                /// Construct a machine with a custom event-queue capacity
                /// `__N`. The channel's capacity must also be `__N`.
                pub fn with_queue_capacity<__M, const __N: usize>(
                    ctx: #ctx_ty,
                    channel: &'static ::embassy_sync::channel::Channel<__M, #ev_ty, __N>,
                ) -> #machine_name<__N>
                where
                    __M: ::embassy_sync::blocking_mutex::raw::RawMutex
                        + ::core::marker::Sync
                        + ::core::marker::Send
                        + 'static,
                    #ev_ty: ::core::marker::Send,
                {
                    let mut __self = <#machine_name<__N>>::with_queue_capacity_internal(ctx);
                    __self.__embassy_rx = Some(channel.receiver().into());
                    __self.__embassy_tx = Some(channel.sender().into());
                    __self
                }
                /// Construct a machine that is driven only via `send()` +
                /// `step().await`. No external `Channel` is wired up, so
                /// [`sender()`](Self::sender) and [`run()`](Self::run)
                /// panic if called. Use this when the single task that
                /// owns the machine is also its only event producer —
                /// the internal queue is sufficient and a
                /// `&'static Channel` would be dead weight.
                pub fn new_local(ctx: #ctx_ty) -> Self {
                    <#machine_name<8>>::with_queue_capacity_internal(ctx)
                }
            }

            #[cfg(feature = "embassy")]
            impl<const __QN: usize> #machine_name<__QN> {
                /// Channel-less variant of
                /// [`with_queue_capacity`](Self::with_queue_capacity)
                /// — see [`new_local`](Self::new_local).
                pub fn with_queue_capacity_local(ctx: #ctx_ty) -> #machine_name<__QN> {
                    <#machine_name<__QN>>::with_queue_capacity_internal(ctx)
                }
            }

            #[cfg(feature = "embassy")]
            impl<const __QN: usize> #machine_name<__QN> {
                /// Obtain a clonable, `Send` handle for pushing events into
                /// this machine from ISRs or other Embassy tasks. The channel
                /// was provided at construction.
                pub fn sender(&self) -> #sender_name {
                    #sender_name {
                        inner: self.__embassy_tx
                            .expect("hsmc: embassy channel (populated by Machine::new)"),
                    }
                }

                /// Run the machine to completion, awaiting events from the
                /// bound channel, firing timers via `embassy_time`, and
                /// polling any active `during:` activities for the current
                /// state path. Returns `Ok(())` on clean termination;
                /// `Err(HsmcError::QueueFull)` if an action's `emit()`
                /// overflowed the internal queue.
                pub async fn run(&mut self) -> ::core::result::Result<(), ::hsmc::HsmcError> {
                    #embassy_run_with_ch_body
                }
            }
        }
    } else {
        // Timer-only machines (no `events:` declared) have no channel. Keep
        // the existing timer-driven run().
        quote! {
            #[cfg(feature = "embassy")]
            impl #machine_name<8> {
                /// Construct a timer-only machine (no `events:` declared).
                pub fn new(ctx: #ctx_ty) -> Self {
                    <#machine_name<8>>::with_queue_capacity_internal(ctx)
                }
                /// Construct a timer-only machine with custom queue capacity.
                pub fn with_queue_capacity<const __N: usize>(ctx: #ctx_ty) -> #machine_name<__N> {
                    <#machine_name<__N>>::with_queue_capacity_internal(ctx)
                }
            }

            #[cfg(feature = "embassy")]
            impl<const __QN: usize> #machine_name<__QN> {
                /// Timer-only run loop (no `events:` declared) using
                /// `embassy_time::Timer`, plus any `during:` activities
                /// declared on the active state path.
                pub async fn run(&mut self) -> ::core::result::Result<(), ::hsmc::HsmcError> {
                    #[allow(dead_code)]
                    enum __HsmcRace<E> { Event(E), ChannelClosed, Timer }
                    if self.terminated { return Err(::hsmc::HsmcError::AlreadyTerminated); }
                    let mut next = self.step(::hsmc::Duration::ZERO).await;
                    if self.take_overflow() { return Err(::hsmc::HsmcError::QueueFull); }
                    let mut last_instant = ::embassy_time::Instant::now();
                    while !self.terminated {
                        if self.take_overflow() { return Err(::hsmc::HsmcError::QueueFull); }
                        if !self.queue.is_empty() {
                            let now = ::embassy_time::Instant::now();
                            let elapsed_us = now.duration_since(last_instant).as_micros();
                            last_instant = now;
                            next = self.step(::hsmc::Duration::from_micros(elapsed_us)).await;
                            continue;
                        }
                        // No queued events. Decide whether to wait.
                        let has_any_during: bool = match self.current {
                            #(#has_active_during_arms)*
                            _ => false,
                        };
                        if next.is_none() && !has_any_during { break; }
                        let race: __HsmcRace<#ev_ty> = match self.current {
                            #(#embassy_race_arms_timer_only)*
                            _ => {
                                // No durings active; just await the timer.
                                match next {
                                    Some(d) => {
                                        ::embassy_time::Timer::after(
                                            ::embassy_time::Duration::from_micros(d.as_micros() as u64)
                                        ).await;
                                    }
                                    None => ::core::future::pending::<()>().await,
                                }
                                __HsmcRace::Timer
                            }
                        };
                        match race {
                            __HsmcRace::Event(ev) => { let _ = self.queue.push_back(ev); }
                            __HsmcRace::ChannelClosed => {}
                            __HsmcRace::Timer => {}
                        }
                        let now = ::embassy_time::Instant::now();
                        let elapsed_us = now.duration_since(last_instant).as_micros();
                        last_instant = now;
                        next = self.step(::hsmc::Duration::from_micros(elapsed_us)).await;
                    }
                    Ok(())
                }
            }
        }
    };

    // Embassy stored receiver/sender fields. Populated at construction time
    // when the user provides an `&'static embassy_sync::channel::Channel`.
    let embassy_fields = if emit_sender {
        quote! {
            #[cfg(feature = "embassy")]
            __embassy_rx: Option<::embassy_sync::channel::SendDynamicReceiver<'static, #ev_ty>>,
            #[cfg(feature = "embassy")]
            __embassy_tx: Option<::embassy_sync::channel::SendDynamicSender<'static, #ev_ty>>,
        }
    } else {
        quote! {}
    };
    let embassy_ctor = if emit_sender {
        quote! {
            #[cfg(feature = "embassy")]
            __embassy_rx: None,
            #[cfg(feature = "embassy")]
            __embassy_tx: None,
        }
    } else {
        quote! {}
    };

    // Eager tokio channel fields. Always emitted when `tokio` feature is
    // compiled in AND this machine has events.
    let tokio_fields = if emit_sender {
        quote! {
            #[cfg(feature = "tokio")]
            __tokio_tx: Option<::tokio::sync::mpsc::UnboundedSender<#ev_ty>>,
            #[cfg(feature = "tokio")]
            __tokio_rx: Option<::tokio::sync::mpsc::UnboundedReceiver<#ev_ty>>,
        }
    } else {
        quote! {}
    };
    let tokio_ctor = if emit_sender {
        quote! {
            #[cfg(feature = "tokio")]
            __tokio_tx: { let (tx, _rx) = ::tokio::sync::mpsc::unbounded_channel(); Some(tx) },
            #[cfg(feature = "tokio")]
            __tokio_rx: None,
        }
    } else {
        quote! {}
    };
    // We need to create tx+rx together; do it via a small helper.
    let tokio_ctor_setup = if emit_sender {
        quote! {
            #[cfg(feature = "tokio")]
            {
                let (tx, rx) = ::tokio::sync::mpsc::unbounded_channel();
                __self.__tokio_tx = Some(tx);
                __self.__tokio_rx = Some(rx);
            }
        }
    } else {
        quote! {}
    };

    quote! {
        #no_events_enum

        #[doc = #state_enum_doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[allow(non_camel_case_types, dead_code)]
        pub enum #state_enum_name {
            #(#state_variants),*
        }

        #[doc = #actions_trait_doc]
        #[cfg(not(any(feature = "tokio", feature = "embassy")))]
        #[allow(non_snake_case)]
        pub trait #actions_trait_name {
            #(#action_methods_sync)*
        }

        #[doc = #actions_trait_doc]
        ///
        /// Under the `tokio` or `embassy` feature, action methods are
        /// `async fn` so that entry/exit/event-action bodies can `.await`
        /// peripheral calls (I2C flush, radio ops, etc.). Returned futures
        /// must be `Send` for compatibility with multi-threaded / Send
        /// executors.
        #[cfg(any(feature = "tokio", feature = "embassy"))]
        #[allow(non_snake_case)]
        pub trait #actions_trait_name {
            #(#action_methods_async)*
        }

        #[doc = #action_ctx_doc]
        pub struct #action_ctx_name<'a> {
            ctx: &'a mut #ctx_ty,
            __queue: &'a mut (dyn ::hsmc::__private::QueuePush<#ev_ty> + ::core::marker::Send),
            __terminated: &'a bool,
            __journal: &'a mut ::hsmc::__private::JournalSink,
        }

        #[doc = #ctx_alias_doc]
        pub type #ctx_alias_name<'a> = #action_ctx_name<'a>;

        impl<'a> ::core::ops::Deref for #action_ctx_name<'a> {
            type Target = #ctx_ty;
            fn deref(&self) -> &#ctx_ty { self.ctx }
        }

        impl<'a> ::core::ops::DerefMut for #action_ctx_name<'a> {
            fn deref_mut(&mut self) -> &mut #ctx_ty { self.ctx }
        }

        impl<'a> #action_ctx_name<'a> {
            /// Push an event into the internal queue. It will be processed
            /// after the current event's handling completes (§2.12). Returns
            /// [`HsmcError::QueueFull`] if the queue is full, or
            /// [`HsmcError::AlreadyTerminated`] if the machine is shutting
            /// down.
            #[allow(unused_variables)]
            pub fn emit(&mut self, event: #ev_ty) -> ::core::result::Result<(), ::hsmc::HsmcError> {
                if *self.__terminated { return Err(::hsmc::HsmcError::AlreadyTerminated); }
                let __ev_id = __event_to_id(&event);
                let __result = self.__queue.push(event);
                if __result.is_ok() {
                    ::hsmc::__chart_observe!(
                        EmitQueued,
                        &mut *self.__journal,
                        #chart_name_lit,
                        __ev_id, __event_name(__ev_id)
                    );
                } else {
                    ::hsmc::__chart_observe!(
                        EmitFailed,
                        &mut *self.__journal,
                        #chart_name_lit,
                        __ev_id, __event_name(__ev_id)
                    );
                }
                __result
            }

            /// Convenience wrapper around [`emit()`](Self::emit) that panics on
            /// failure (§12.1). Use in contexts where a full queue is a bug
            /// rather than a recoverable condition.
            pub fn emit_or_panic(&mut self, event: #ev_ty) {
                self.emit(event).expect("hsmc: event queue full");
            }
        }

        #[doc = #machine_doc]
        #[allow(non_snake_case, non_camel_case_types)]
        pub struct #machine_name<const __QN: usize = 8> {
            ctx: #ctx_ty,
            current: Option<u16>,
            queue: ::hsmc::__private::heapless::Deque<#ev_ty, __QN>,
            timers: ::hsmc::__private::TimerTable<#max_timers>,
            terminated: bool,
            __overflow: bool,
            /// Deterministic execution journal. ZST when the `hsmc/journal`
            /// feature is off; a `Vec<TraceEvent>` when on.
            __journal: ::hsmc::__private::JournalSink,
            #tokio_fields
            #embassy_fields
        }

        const __PARENT: [Option<u16>; #n_states] = [#(#parent_table),*];
        const __DEPTH: [u8; #n_states] = [#(#depth_table),*];
        const __DEFAULT_CHILD: [Option<u16>; #n_states] = [#(#default_child_table),*];

        #name_helpers

        #[allow(dead_code, unreachable_patterns)]
        fn __state_id_to_public(id: u16) -> #state_enum_name {
            match id {
                #(#state_id_to_variant_arms)*
                _ => panic!("internal: state id {} has no public variant", id),
            }
        }

        #[allow(unreachable_code, unused_variables)]
        fn __duration_for(tid: u16) -> ::hsmc::Duration {
            match tid {
                #(#duration_expr_arms)*
                _ => ::hsmc::Duration::ZERO,
            }
        }

        #[allow(unreachable_code, unused_variables)]
        fn __duration_repeats(tid: u16) -> bool {
            match tid {
                #(#duration_repeat_arms)*
                _ => false,
            }
        }

        #[allow(unreachable_patterns, unused_variables)]
        fn __event_to_id(ev: &#ev_ty) -> u16 {
            match ev {
                #(#event_variant_arms)*
                _ => u16::MAX,
            }
        }

        #[allow(unused_variables)]
        fn __handler_lookup(
            state_id: u16,
            trigger_kind: u8,
            trigger_id: u16,
        ) -> Option<(&'static [u16], Option<u16>)> {
            match state_id {
                #(#state_handler_arms)*
                _ => {}
            }
            None
        }

        #[allow(unused_variables)]
        fn __owned_timers(state_id: u16) -> &'static [u16] {
            match state_id {
                #(#owned_timers_arms)*
                _ => &[],
            }
        }

        enum TerminateCheck { Yes, No }

        #[allow(unused_variables)]
        fn __check_terminate(__ev: &#ev_ty) -> TerminateCheck {
            #terminate_match
            TerminateCheck::No
        }

        impl #machine_name<8> {
            /// Construct a new machine with the default event-queue capacity of 8 (§4.4).
            #[cfg(not(feature = "embassy"))]
            pub fn new(ctx: #ctx_ty) -> Self {
                <#machine_name<8>>::with_queue_capacity_internal(ctx)
            }
            /// Construct a machine with a custom event-queue capacity `__N` (§5.2).
            #[cfg(not(feature = "embassy"))]
            pub fn with_queue_capacity<const __N: usize>(ctx: #ctx_ty) -> #machine_name<__N> {
                <#machine_name<__N>>::with_queue_capacity_internal(ctx)
            }
        }

        impl<const __QN: usize> #machine_name<__QN> {
            /// ASCII diagram of the state hierarchy, default children, entry/
            /// exit actions, `during:` activities, and handler triggers.
            /// Intended for `defmt::info!`, panic messages, and docs.
            ///
            /// Accessible as `Self::STATE_CHART` from any impl block on the
            /// machine; externally requires the turbofish (for example
            /// `Machine::<8>::STATE_CHART`) since the machine carries a const
            /// generic queue capacity.
            pub const STATE_CHART: &'static str = #state_chart_str;

            fn with_queue_capacity_internal(ctx: #ctx_ty) -> Self {
                #[allow(unused_mut)]
                let mut __self = Self {
                    ctx,
                    current: None,
                    queue: ::hsmc::__private::heapless::Deque::new(),
                    timers: ::hsmc::__private::TimerTable::new(),
                    terminated: false,
                    __overflow: false,
                    __journal: ::hsmc::__private::JournalSink::new(),
                    #tokio_ctor
                    #embassy_ctor
                };
                #tokio_ctor_setup
                __self
            }

            /// Returns and clears the internal-emit overflow flag. Set by
            /// the generated action dispatch when `emit()` inside an action
            /// fails to enqueue — `run()` consults this to surface
            /// [`HsmcError::QueueFull`] out of the async loop rather than
            /// silently dropping the event. Exposed for callers driving the
            /// machine via `step()` directly.
            pub fn take_overflow(&mut self) -> bool {
                let v = self.__overflow;
                self.__overflow = false;
                v
            }

            /// Returns `true` once the machine has processed its `terminate` event (§4.4).
            pub fn is_terminated(&self) -> bool { self.terminated }

            /// Return the current leaf state. Panics if called before the
            /// first `step()` (§4.4).
            pub fn current_state(&self) -> #state_enum_name {
                __state_id_to_public(self.current.expect("machine not started; call step() first"))
            }

            /// Returns `true` when internal event queue has pending events
            /// (§12.3). Useful for schedulers that want to poll `step()`
            /// again eagerly.
            pub fn has_pending_events(&self) -> bool {
                !self.queue.is_empty()
            }

            /// Consume the machine and yield the user-provided context (§4.4).
            pub fn into_context(self) -> #ctx_ty { self.ctx }

            /// Borrow the user-provided context (extension beyond spec §4.4; provided for test convenience).
            pub fn context(&self) -> &#ctx_ty { &self.ctx }
            /// Mutably borrow the user-provided context (extension beyond spec §4.4; provided for test convenience).
            pub fn context_mut(&mut self) -> &mut #ctx_ty { &mut self.ctx }

            /// Borrow the deterministic execution journal. Only available
            /// under `feature = "journal"`. Returns the in-order sequence
            /// of every observable atom — entries, exits, action calls,
            /// during start/cancel, timer arm/cancel/fire, queued emits,
            /// event delivery, transitions, and termination.
            #[cfg(feature = "journal")]
            pub fn journal(&self) -> &[::hsmc::TraceEvent] {
                self.__journal.events()
            }

            /// Take and clear the deterministic execution journal. Useful
            /// at test boundaries to compare against an expected sequence
            /// without lingering events from earlier runs.
            #[cfg(feature = "journal")]
            pub fn take_journal(&mut self) -> ::hsmc::Journal {
                self.__journal.take()
            }

            /// Drop all journal events without returning them.
            #[cfg(feature = "journal")]
            pub fn clear_journal(&mut self) {
                self.__journal.clear();
            }

            /// Stable identifier for this chart's structural definition.
            /// A different `CHART_HASH` means the chart is a different
            /// machine — replay against a journal recorded for a
            /// different `CHART_HASH` is meaningless and the test driver
            /// should refuse to compare.
            pub const CHART_HASH: u64 = #chart_hash_lit;

            /// Returns `CHART_HASH` (instance-method form for symmetry
            /// with other introspection methods).
            pub const fn chart_hash(&self) -> u64 { Self::CHART_HASH }

            /// Advance the machine by `elapsed` wall-clock time. Returns the
            /// duration until the next timer fires, or `None` if the machine
            /// has no live timers (§4.4).
            ///
            /// Under the `tokio` or `embassy` feature, `step()` is `async`
            /// because action methods are `async fn` and must be `.await`ed
            /// during dispatch. Under default features it stays sync.
            #[cfg(not(any(feature = "tokio", feature = "embassy")))]
            #[allow(unused_variables)]
            pub fn step(&mut self, elapsed: ::hsmc::Duration) -> Option<::hsmc::Duration> {
                if self.terminated { return None; }

                if self.current.is_none() {
                    #observe_started
                    self.enter_path(0);
                    return self.timers.min_remaining();
                }

                self.timers.decrement(elapsed);

                if let Some((state, trig)) = self.timers.pop_expired(&__DEPTH) {
                    let __t_state = state;
                    let __t_trigger = trig;
                    #observe_timer_fired
                    if __duration_repeats(trig) {
                        let d = __duration_for(trig);
                        self.timers.start(state, trig, d);
                        let sid = state;
                        let tid = trig;
                        #observe_timer_armed
                    }
                    self.dispatch_trigger(state, 1, trig);
                    return self.timers.min_remaining();
                }

                if let Some(ev) = self.queue.pop_front() {
                    let __ev_id = __event_to_id(&ev);
                    #observe_event_received
                    if matches!(__check_terminate(&ev), TerminateCheck::Yes) {
                        #observe_terminate_requested
                        self.do_terminate();
                        return None;
                    }
                    if __ev_id != u16::MAX {
                        let mut __cur = self.current;
                        let mut __delivered = false;
                        while let Some(s) = __cur {
                            if let Some((actions, target)) = __handler_lookup(s, 0, __ev_id) {
                                let __handler_state = s;
                                #observe_event_delivered
                                self.run_handlers(
                                    s, actions, target, Some(&ev),
                                    1u8, __ev_id, 0u16,
                                );
                                __delivered = true;
                                break;
                            }
                            __cur = __PARENT[s as usize];
                        }
                        if !__delivered {
                            #observe_event_dropped
                        }
                    } else {
                        #observe_event_dropped
                    }
                    return self.timers.min_remaining();
                }

                self.timers.min_remaining()
            }

            #[cfg(any(feature = "tokio", feature = "embassy"))]
            #[allow(unused_variables)]
            pub async fn step(&mut self, elapsed: ::hsmc::Duration) -> Option<::hsmc::Duration> {
                if self.terminated { return None; }

                if self.current.is_none() {
                    #observe_started
                    self.enter_path(0).await;
                    return self.timers.min_remaining();
                }

                self.timers.decrement(elapsed);

                if let Some((state, trig)) = self.timers.pop_expired(&__DEPTH) {
                    let __t_state = state;
                    let __t_trigger = trig;
                    #observe_timer_fired
                    if __duration_repeats(trig) {
                        let d = __duration_for(trig);
                        self.timers.start(state, trig, d);
                        let sid = state;
                        let tid = trig;
                        #observe_timer_armed
                    }
                    self.dispatch_trigger(state, 1, trig).await;
                    return self.timers.min_remaining();
                }

                if let Some(ev) = self.queue.pop_front() {
                    let __ev_id = __event_to_id(&ev);
                    #observe_event_received
                    if matches!(__check_terminate(&ev), TerminateCheck::Yes) {
                        #observe_terminate_requested
                        self.do_terminate().await;
                        return None;
                    }
                    if __ev_id != u16::MAX {
                        let mut __cur = self.current;
                        let mut __delivered = false;
                        while let Some(s) = __cur {
                            if let Some((actions, target)) = __handler_lookup(s, 0, __ev_id) {
                                let __handler_state = s;
                                #observe_event_delivered
                                self.run_handlers(
                                    s, actions, target, Some(&ev),
                                    1u8, __ev_id, 0u16,
                                ).await;
                                __delivered = true;
                                break;
                            }
                            __cur = __PARENT[s as usize];
                        }
                        if !__delivered {
                            #observe_event_dropped
                        }
                    } else {
                        #observe_event_dropped
                    }
                    return self.timers.min_remaining();
                }

                self.timers.min_remaining()
            }

            /// Push an event directly into the machine's queue (synchronous,
            /// single-threaded). Extension beyond spec §4.4 — the canonical
            /// cross-thread path is [`sender().send()`](Self::sender). Useful
            /// for tests and tight `step()` driver loops.
            #[doc(hidden)]
            pub fn send(&mut self, event: #ev_ty) -> ::core::result::Result<(), ::hsmc::HsmcError> {
                if self.terminated { return Err(::hsmc::HsmcError::AlreadyTerminated); }
                self.queue.push_back(event).map_err(|_| ::hsmc::HsmcError::QueueFull)
            }

            /// Push an event into the internal queue and drain the queue to
            /// quiescence before returning. Under an async feature the step
            /// loop awaits action futures; after `dispatch()` returns,
            /// `current_state()` reflects the post-dispatch state. Use this
            /// from the task that owns the machine when you want "event
            /// processed" semantics (a drop-in replacement for the
            /// `send + drain` pattern). For cross-task / ISR injection use
            /// [`sender()`](Self::sender) instead.
            #[cfg(any(feature = "tokio", feature = "embassy"))]
            pub async fn dispatch(&mut self, event: #ev_ty) -> ::core::result::Result<(), ::hsmc::HsmcError> {
                if self.terminated { return Err(::hsmc::HsmcError::AlreadyTerminated); }
                self.queue.push_back(event).map_err(|_| ::hsmc::HsmcError::QueueFull)?;
                // Prime state on first call.
                if self.current.is_none() {
                    let _ = self.step(::hsmc::Duration::ZERO).await;
                    if self.take_overflow() { return Err(::hsmc::HsmcError::QueueFull); }
                    if self.terminated { return Ok(()); }
                }
                while !self.queue.is_empty() {
                    let _ = self.step(::hsmc::Duration::ZERO).await;
                    if self.take_overflow() { return Err(::hsmc::HsmcError::QueueFull); }
                    if self.terminated { return Ok(()); }
                }
                Ok(())
            }

            // Reason kind passed by callers to `run_handlers`:
            //   1 → event-driven (reason_a = event id)
            //   2 → timer-driven (reason_a = state id, reason_b = timer id)
            //   0 → internal/other (unused today; reserved)
            #[cfg(not(any(feature = "tokio", feature = "embassy")))]
            #[allow(unused_variables, clippy::too_many_arguments)]
            fn run_handlers(
                &mut self,
                source_state: u16,
                actions: &'static [u16],
                target: Option<u16>,
                event: Option<&#ev_ty>,
                __reason_kind: u8,
                __reason_a: u16,
                __reason_b: u16,
            ) {
                let __handler_state = source_state;
                for &aid in actions {
                    #observe_action_handler
                    self.run_action(aid, event);
                    if self.terminated { return; }
                }
                if let Some(target) = target {
                    match __reason_kind {
                        1 => {
                            let __ev_id = __reason_a;
                            #observe_transition_fired_event
                        }
                        2 => {
                            let __t_state = __reason_a;
                            let __t_trigger = __reason_b;
                            #observe_transition_fired_timer
                        }
                        _ => {}
                    }
                    let __t_complete_from = self.current;
                    self.transition(source_state, target);
                    #observe_transition_complete
                }
            }

            #[cfg(any(feature = "tokio", feature = "embassy"))]
            #[allow(unused_variables, clippy::too_many_arguments)]
            async fn run_handlers(
                &mut self,
                source_state: u16,
                actions: &'static [u16],
                target: Option<u16>,
                event: Option<&#ev_ty>,
                __reason_kind: u8,
                __reason_a: u16,
                __reason_b: u16,
            ) {
                let __handler_state = source_state;
                for &aid in actions {
                    #observe_action_handler
                    self.run_action(aid, event).await;
                    if self.terminated { return; }
                }
                if let Some(target) = target {
                    match __reason_kind {
                        1 => {
                            let __ev_id = __reason_a;
                            #observe_transition_fired_event
                        }
                        2 => {
                            let __t_state = __reason_a;
                            let __t_trigger = __reason_b;
                            #observe_transition_fired_timer
                        }
                        _ => {}
                    }
                    let __t_complete_from = self.current;
                    self.transition(source_state, target).await;
                    #observe_transition_complete
                }
            }

            #[cfg(not(any(feature = "tokio", feature = "embassy")))]
            fn run_action(&mut self, aid: u16, event: Option<&#ev_ty>) {
                // Trace + journal emission for action calls now happens at
                // the call sites (`#observe_action_entry`,
                // `#observe_action_exit`, `#observe_action_handler`) so the
                // ActionKind tag and call-site state id can be recorded
                // accurately. `run_action` itself is just dispatch.
                let ctx = &mut self.ctx;
                let mut __proxy = ::hsmc::__private::EmitProxy {
                    queue: &mut self.queue,
                    overflow: &mut self.__overflow,
                };
                let queue: &mut (dyn ::hsmc::__private::QueuePush<#ev_ty> + ::core::marker::Send) = &mut __proxy;
                let terminated = &self.terminated;
                let journal = &mut self.__journal;
                let mut actx = #action_ctx_name {
                    ctx,
                    __queue: queue,
                    __terminated: terminated,
                    __journal: journal,
                };
                fn __dispatch<Ctx: #actions_trait_name>(
                    ctx: &mut Ctx,
                    aid: u16,
                    __event: Option<&#ev_ty>,
                ) {
                    let _ = __event;
                    match aid {
                        #(#run_action_arms_sync)*
                        _ => {}
                    }
                }
                __dispatch(&mut actx, aid, event);
            }

            #[cfg(any(feature = "tokio", feature = "embassy"))]
            async fn run_action(&mut self, aid: u16, event: Option<&#ev_ty>) {
                // See sync run_action above — observation happens at call sites.
                let ctx = &mut self.ctx;
                let mut __proxy = ::hsmc::__private::EmitProxy {
                    queue: &mut self.queue,
                    overflow: &mut self.__overflow,
                };
                let queue: &mut (dyn ::hsmc::__private::QueuePush<#ev_ty> + ::core::marker::Send) = &mut __proxy;
                let terminated = &self.terminated;
                let journal = &mut self.__journal;
                let mut actx = #action_ctx_name {
                    ctx,
                    __queue: queue,
                    __terminated: terminated,
                    __journal: journal,
                };
                async fn __dispatch<Ctx: #actions_trait_name>(
                    ctx: &mut Ctx,
                    aid: u16,
                    __event: Option<&#ev_ty>,
                ) {
                    let _ = __event;
                    match aid {
                        #(#run_action_arms_async)*
                        _ => {}
                    }
                }
                __dispatch(&mut actx, aid, event).await;
            }

            // §2.6: classify transitions by the relationship between the
            // innermost active state `I` and the target `T`:
            //   - Up-transition: T is already active (T is a strict ancestor
            //     of I). Unwind the subtree strictly below T; do NOT exit or
            //     re-enter T; do NOT descend into defaults. You cannot enter
            //     a state you never left.
            //   - Self-transition (T == I): external semantics — exit T and
            //     re-enter it, then descend into defaults if T has children.
            //   - Lateral: standard LCA semantics.
            fn is_up_transition(current: Option<u16>, target: u16) -> bool {
                let Some(mut node) = current else { return false; };
                if node == target { return false; }
                while let Some(p) = __PARENT[node as usize] {
                    if p == target { return true; }
                    node = p;
                }
                false
            }

            #[cfg(not(any(feature = "tokio", feature = "embassy")))]
            fn transition(&mut self, _source_matched_state: u16, target: u16) {
                if Self::is_up_transition(self.current, target) {
                    if let Some(mut cur) = self.current {
                        while cur != target {
                            self.exit_state(cur);
                            match __PARENT[cur as usize] {
                                Some(p) => cur = p,
                                None => break,
                            }
                        }
                    }
                    self.current = Some(target);
                    return;
                }

                let mut lca = Self::lca(self.current.unwrap_or(target), target);
                if self.current == Some(target) {
                    // Self-transition: bump LCA so target is exited and re-entered.
                    lca = __PARENT[target as usize];
                }
                if let Some(mut cur) = self.current {
                    loop {
                        if Some(cur) == lca { break; }
                        self.exit_state(cur);
                        match __PARENT[cur as usize] {
                            Some(p) => cur = p,
                            None => break,
                        }
                    }
                }
                let mut path: ::hsmc::__private::heapless::Vec<u16, 16> =
                    ::hsmc::__private::heapless::Vec::new();
                let mut node = Some(target);
                while node != lca && node.is_some() {
                    let n = node.unwrap();
                    let _ = path.push(n);
                    node = __PARENT[n as usize];
                }
                for i in (0..path.len()).rev() {
                    let s = path[i];
                    self.enter_state_no_descent(s);
                }
                self.descend_defaults(target);
            }

            #[cfg(any(feature = "tokio", feature = "embassy"))]
            async fn transition(&mut self, _source_matched_state: u16, target: u16) {
                if Self::is_up_transition(self.current, target) {
                    if let Some(mut cur) = self.current {
                        while cur != target {
                            self.exit_state(cur).await;
                            match __PARENT[cur as usize] {
                                Some(p) => cur = p,
                                None => break,
                            }
                        }
                    }
                    self.current = Some(target);
                    return;
                }

                let mut lca = Self::lca(self.current.unwrap_or(target), target);
                if self.current == Some(target) {
                    lca = __PARENT[target as usize];
                }
                if let Some(mut cur) = self.current {
                    loop {
                        if Some(cur) == lca { break; }
                        self.exit_state(cur).await;
                        match __PARENT[cur as usize] {
                            Some(p) => cur = p,
                            None => break,
                        }
                    }
                }
                let mut path: ::hsmc::__private::heapless::Vec<u16, 16> =
                    ::hsmc::__private::heapless::Vec::new();
                let mut node = Some(target);
                while node != lca && node.is_some() {
                    let n = node.unwrap();
                    let _ = path.push(n);
                    node = __PARENT[n as usize];
                }
                for i in (0..path.len()).rev() {
                    let s = path[i];
                    self.enter_state_no_descent(s).await;
                }
                self.descend_defaults(target).await;
            }

            fn lca(a: u16, b: u16) -> Option<u16> {
                let mut set: ::hsmc::__private::heapless::Vec<u16, 32> =
                    ::hsmc::__private::heapless::Vec::new();
                let mut cur = Some(a);
                while let Some(x) = cur {
                    let _ = set.push(x);
                    cur = __PARENT[x as usize];
                }
                let mut cur = Some(b);
                while let Some(x) = cur {
                    if set.contains(&x) { return Some(x); }
                    cur = __PARENT[x as usize];
                }
                None
            }

            #[cfg(not(any(feature = "tokio", feature = "embassy")))]
            fn enter_path(&mut self, root: u16) {
                self.enter_state_no_descent(root);
                self.descend_defaults(root);
            }

            #[cfg(any(feature = "tokio", feature = "embassy"))]
            async fn enter_path(&mut self, root: u16) {
                self.enter_state_no_descent(root).await;
                self.descend_defaults(root).await;
            }

            #[cfg(not(any(feature = "tokio", feature = "embassy")))]
            #[allow(unused_variables)]
            fn enter_state_no_descent(&mut self, sid: u16) {
                #observe_enter_began
                let entries = Self::entries_of(sid);
                for &aid in entries {
                    #observe_action_entry
                    self.run_action(aid, None);
                    if self.terminated { return; }
                }
                for &tid in __owned_timers(sid) {
                    let d = __duration_for(tid);
                    self.timers.start(sid, tid, d);
                    #observe_timer_armed
                }
                self.current = Some(sid);
                #observe_durings_started
                #observe_entered
            }

            #[cfg(any(feature = "tokio", feature = "embassy"))]
            #[allow(unused_variables)]
            async fn enter_state_no_descent(&mut self, sid: u16) {
                #observe_enter_began
                let entries = Self::entries_of(sid);
                for &aid in entries {
                    #observe_action_entry
                    self.run_action(aid, None).await;
                    if self.terminated { return; }
                }
                for &tid in __owned_timers(sid) {
                    let d = __duration_for(tid);
                    self.timers.start(sid, tid, d);
                    #observe_timer_armed
                }
                self.current = Some(sid);
                #observe_durings_started
                #observe_entered
            }

            #[cfg(not(any(feature = "tokio", feature = "embassy")))]
            fn descend_defaults(&mut self, sid: u16) {
                let mut cur = sid;
                while let Some(child) = __DEFAULT_CHILD[cur as usize] {
                    self.enter_state_no_descent(child);
                    cur = child;
                }
            }

            #[cfg(any(feature = "tokio", feature = "embassy"))]
            async fn descend_defaults(&mut self, sid: u16) {
                let mut cur = sid;
                while let Some(child) = __DEFAULT_CHILD[cur as usize] {
                    self.enter_state_no_descent(child).await;
                    cur = child;
                }
            }

            #[cfg(not(any(feature = "tokio", feature = "embassy")))]
            #[allow(unused_variables)]
            fn exit_state(&mut self, sid: u16) {
                // Per spec §"Entry and exit ordering":
                //   1. ExitBegan  2. cancel durings  3. cancel timers
                //   4. exit actions  5. Exited
                #observe_exit_began
                #observe_durings_cancelled
                for &tid in __owned_timers(sid) {
                    #observe_timer_cancelled
                }
                self.timers.cancel_state(sid);
                let exits = Self::exits_of(sid);
                for &aid in exits {
                    #observe_action_exit
                    self.run_action(aid, None);
                    if self.terminated { return; }
                }
                #observe_exited
            }

            #[cfg(any(feature = "tokio", feature = "embassy"))]
            #[allow(unused_variables)]
            async fn exit_state(&mut self, sid: u16) {
                #observe_exit_began
                #observe_durings_cancelled
                for &tid in __owned_timers(sid) {
                    #observe_timer_cancelled
                }
                self.timers.cancel_state(sid);
                let exits = Self::exits_of(sid);
                for &aid in exits {
                    #observe_action_exit
                    self.run_action(aid, None).await;
                    if self.terminated { return; }
                }
                #observe_exited
            }

            fn entries_of(sid: u16) -> &'static [u16] {
                match sid {
                    #(#entries_of_arms)*
                    _ => &[],
                }
            }
            fn exits_of(sid: u16) -> &'static [u16] {
                match sid {
                    #(#exits_of_arms)*
                    _ => &[],
                }
            }
            #[doc(hidden)]
            #[allow(unused_variables)]
            fn __durings_count(sid: u16) -> u16 {
                match sid {
                    #(#durings_count_arms)*
                    _ => 0,
                }
            }

            #[cfg(not(any(feature = "tokio", feature = "embassy")))]
            fn do_terminate(&mut self) {
                if let Some(mut cur) = self.current {
                    loop {
                        self.exit_state(cur);
                        match __PARENT[cur as usize] {
                            Some(p) => cur = p,
                            None => break,
                        }
                    }
                }
                self.terminated = true;
                self.current = None;
                while self.queue.pop_front().is_some() {}
                #observe_terminated
            }

            #[cfg(any(feature = "tokio", feature = "embassy"))]
            async fn do_terminate(&mut self) {
                if let Some(mut cur) = self.current {
                    loop {
                        self.exit_state(cur).await;
                        match __PARENT[cur as usize] {
                            Some(p) => cur = p,
                            None => break,
                        }
                    }
                }
                self.terminated = true;
                self.current = None;
                while self.queue.pop_front().is_some() {}
                #observe_terminated
            }

            #[cfg(not(any(feature = "tokio", feature = "embassy")))]
            fn dispatch_trigger(&mut self, state: u16, kind: u8, trig: u16) {
                if let Some((actions, target)) = __handler_lookup(state, kind, trig) {
                    // Timer-driven dispatch: no event payload, reason = timer.
                    self.run_handlers(state, actions, target, None, 2u8, state, trig);
                }
            }

            #[cfg(any(feature = "tokio", feature = "embassy"))]
            async fn dispatch_trigger(&mut self, state: u16, kind: u8, trig: u16) {
                if let Some((actions, target)) = __handler_lookup(state, kind, trig) {
                    self.run_handlers(state, actions, target, None, 2u8, state, trig).await;
                }
            }
        }

        #default_sender_impl
        #tokio_sender_impl
        #embassy_sender_impl
    }
}

#[allow(dead_code)]
fn _unused(_: Span, _: &Ident) {}
