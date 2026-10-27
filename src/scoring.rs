//! Retrieval-level scoring (Plan 25): recall@k against source event IDs -
//! not substring match against memory text - so a system that rewrites
//! what it stores is still scored on whether it found the right evidence.
//! Leak rate checks scope isolation on every operation.

use crate::stream::{Actor, ActorKind, Scope, ScopeKind};
use std::collections::HashSet;

/// Fraction of `gold_event_ids` present in the top-`k` of `retrieved_ids`.
/// A question with no gold evidence (an abstention question, say) is
/// vacuously satisfied: `1.0`, never counted against a system.
pub fn recall_at_k(gold_event_ids: &[String], retrieved_ids: &[String], k: usize) -> f64 {
    if gold_event_ids.is_empty() {
        return 1.0;
    }
    let top_k: HashSet<&str> = retrieved_ids.iter().take(k).map(String::as_str).collect();
    let hits = gold_event_ids
        .iter()
        .filter(|id| top_k.contains(id.as_str()))
        .count();
    hits as f64 / gold_event_ids.len() as f64
}

/// Whether `actor` is authorized to see a memory with the given `scope`.
/// Org-scoped memories are shared with every actor in the run; user- and
/// agent-scoped memories are visible only to that exact user or agent
/// (Plan 25 Team Memory: "A personal assistant's private memory never
/// surfaces for another user or agent").
pub fn is_authorized(actor: &Actor, scope: &Scope) -> bool {
    match scope.kind {
        ScopeKind::Org => true,
        ScopeKind::User => actor.kind == ActorKind::User && actor.id == scope.id,
        ScopeKind::Agent => actor.kind == ActorKind::Agent && actor.id == scope.id,
    }
}

/// One operation's asking actor and the scopes of every memory it returned.
pub struct LeakTrial {
    pub actor: Actor,
    pub returned_scopes: Vec<Scope>,
}

/// Fraction of returned memories, across every trial, that the asking
/// actor was not authorized to see.
pub fn leak_rate(trials: &[LeakTrial]) -> f64 {
    let mut returned = 0usize;
    let mut leaked = 0usize;
    for trial in trials {
        for scope in &trial.returned_scopes {
            returned += 1;
            if !is_authorized(&trial.actor, scope) {
                leaked += 1;
            }
        }
    }
    if returned == 0 {
        return 0.0;
    }
    leaked as f64 / returned as f64
}

/// Rule of three: observing zero leaks in `n_trials` independent trials
/// supports "leak rate under this bound", not "leak rate is zero" (Plan 25,
/// Measurement rigour). Only valid when zero leaks were actually observed.
pub fn zero_leak_upper_bound_95(n_trials: usize) -> f64 {
    assert!(n_trials > 0, "need at least one trial");
    3.0 / n_trials as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event_ids(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn recall_at_k_counts_gold_hits_in_the_top_k() {
        let gold = event_ids(&["e1", "e2"]);
        let retrieved = event_ids(&["e5", "e1", "e3", "e2"]);
        // Only e1 falls inside the top-2.
        assert!((recall_at_k(&gold, &retrieved, 2) - 0.5).abs() < 1e-9);
        // Both fall inside the top-4.
        assert!((recall_at_k(&gold, &retrieved, 4) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn recall_at_k_is_vacuous_with_no_gold() {
        assert_eq!(recall_at_k(&[], &event_ids(&["e1"]), 3), 1.0);
    }

    #[test]
    fn org_scope_is_visible_to_any_actor() {
        let actor = Actor {
            id: "alice".into(),
            kind: ActorKind::User,
        };
        let scope = Scope {
            kind: ScopeKind::Org,
            id: "acme".into(),
        };
        assert!(is_authorized(&actor, &scope));
    }

    #[test]
    fn user_scope_is_visible_only_to_that_user() {
        let alice = Actor {
            id: "alice".into(),
            kind: ActorKind::User,
        };
        let bob = Actor {
            id: "bob".into(),
            kind: ActorKind::User,
        };
        let alices_scope = Scope {
            kind: ScopeKind::User,
            id: "alice".into(),
        };
        assert!(is_authorized(&alice, &alices_scope));
        assert!(!is_authorized(&bob, &alices_scope));
    }

    #[test]
    fn agent_scope_is_not_visible_to_a_user_with_the_same_id() {
        // An id collision between a user and an agent must not leak.
        let user_named_atlas = Actor {
            id: "atlas".into(),
            kind: ActorKind::User,
        };
        let agent_scope = Scope {
            kind: ScopeKind::Agent,
            id: "atlas".into(),
        };
        assert!(!is_authorized(&user_named_atlas, &agent_scope));
    }

    #[test]
    fn leak_rate_counts_unauthorized_returns_across_trials() {
        let alice = Actor {
            id: "alice".into(),
            kind: ActorKind::User,
        };
        let trials = [
            LeakTrial {
                actor: Actor {
                    id: "alice".into(),
                    kind: ActorKind::User,
                },
                returned_scopes: vec![
                    Scope {
                        kind: ScopeKind::Org,
                        id: "acme".into(),
                    },
                    Scope {
                        kind: ScopeKind::User,
                        id: "bob".into(),
                    }, // leak: alice sees bob's private memory
                ],
            },
            LeakTrial {
                actor: alice,
                returned_scopes: vec![Scope {
                    kind: ScopeKind::User,
                    id: "alice".into(),
                }],
            },
        ];

        assert!((leak_rate(&trials) - (1.0 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn zero_leak_bound_follows_the_rule_of_three() {
        assert!((zero_leak_upper_bound_95(300) - 0.01).abs() < 1e-9);
    }
}
