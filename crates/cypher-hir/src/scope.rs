//! Scope graph and resolved-names table (spec 0001 §6.2).
//!
//! # Design
//!
//! A `ScopeGraph` is a forest of `ScopeNode`s connected by parent-child
//! edges. Every lexical region that introduces or restricts bindings
//! corresponds to one `ScopeNode`:
//!
//! - `Root` — statement root; parent of all top-level scopes.
//! - `Match` — MATCH / OPTIONAL MATCH clause; binds pattern variables.
//! - `Create` / `Merge` — CREATE / MERGE clause; also binds pattern vars.
//! - `Unwind` — UNWIND … AS v clause; binds one Value variable.
//! - `Call` — CALL … YIELD …; binds yield columns.
//! - `WithBarrier` — WITH clause; acts as a scope barrier (§6.2):
//!   only explicitly projected names survive into the next scope.
//! - `Return` — RETURN clause; terminal, produces output signature.
//!
//! ## WITH barrier semantics
//!
//! When a `WithBarrier` scope is encountered during resolution, only the
//! names listed in its `projected` set are visible to scopes that
//! follow. All earlier bindings that are not re-projected fall out of
//! scope. This is enforced by `ScopeGraph::resolve_at`: the lookup walks
//! up the parent chain but stops at a `WithBarrier` boundary, only
//! passing through names that appear in the barrier's projection list.
//!
//! ## Shadowing
//!
//! If a name is bound a second time in the same (or an inner) scope, the
//! new binding shadows the old one. The resolver records both the new
//! binding and the shadowed `VarId` so that the semantic pass can emit
//! `W6004` (or the caller can choose to warn via `E1002`).

use indexmap::IndexMap;
use smol_str::SmolStr;

use crate::{VarId, VarKind};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Identifies a node within a `ScopeGraph`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopeId(pub u32);

/// Classification of a lexical scope (spec §6.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeKind {
    /// Statement root; all other scopes descend from here.
    Root,
    /// MATCH / OPTIONAL MATCH clause.
    Match { optional: bool },
    /// CREATE clause.
    Create,
    /// MERGE clause.
    Merge,
    /// UNWIND … AS v clause.
    Unwind,
    /// CALL … YIELD … clause.
    Call,
    /// WITH clause — scope barrier. Only `projected` names pass through.
    ///
    /// `projected` is a list of `(projected_name, source_var_id)` pairs.
    /// `projected_name` is the alias if present, otherwise the original
    /// variable name. This models `WITH a AS b` (b is visible, a is not).
    WithBarrier { projected: Vec<(SmolStr, VarId)> },
    /// RETURN clause — terminal output scope.
    Return,
}

/// A single node in the scope forest.
#[derive(Debug, Clone)]
pub struct ScopeNode {
    pub id: ScopeId,
    pub kind: ScopeKind,
    /// Parent scope, if any. The root scope has no parent.
    pub parent: Option<ScopeId>,
    /// Bindings introduced directly in this scope.
    /// Maps name → (`VarId`, optional shadowed-`VarId`).
    pub bindings: IndexMap<SmolStr, (VarId, VarKind, Option<VarId>)>,
    /// Child scopes, in order of introduction.
    pub children: Vec<ScopeId>,
}

/// The full scope forest for one HIR statement (spec §6.2).
#[derive(Debug, Clone, Default)]
pub struct ScopeGraph {
    nodes: Vec<ScopeNode>,
}

impl ScopeGraph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a new scope node to the graph and return its `ScopeId`.
    pub fn add_scope(&mut self, kind: ScopeKind, parent: Option<ScopeId>) -> ScopeId {
        let id = ScopeId(u32::try_from(self.nodes.len()).expect("scope count overflowed u32"));
        if let Some(parent_id) = parent {
            self.nodes[parent_id.0 as usize].children.push(id);
        }
        self.nodes.push(ScopeNode {
            id,
            kind,
            parent,
            bindings: IndexMap::new(),
            children: Vec::new(),
        });
        id
    }

    /// Introduce a binding into `scope`.  Returns the previous `VarId`
    /// bound to the same name (for shadowing diagnostics), if any.
    pub fn bind(
        &mut self,
        scope: ScopeId,
        name: SmolStr,
        var: VarId,
        kind: VarKind,
    ) -> Option<VarId> {
        let node = &mut self.nodes[scope.0 as usize];
        let shadowed = node.bindings.get(&name).map(|(v, _, _)| *v);
        node.bindings.insert(name, (var, kind, shadowed));
        shadowed
    }

    /// Resolve `name` starting from `scope`, walking up the parent chain.
    ///
    /// Respects `WithBarrier` semantics: when a barrier is encountered the
    /// walk continues upward only if `name` appears in the barrier's
    /// `projected` list. Returns `None` if no binding is found.
    pub fn resolve_at(&self, scope: ScopeId, name: &str) -> Option<VarId> {
        let mut current = scope;
        loop {
            let node = &self.nodes[current.0 as usize];
            // 1. Check this scope's own bindings.
            if let Some((var, _, _)) = node.bindings.get(name) {
                return Some(*var);
            }
            // 2. Move to parent, subject to WITH barrier.
            match &node.kind {
                ScopeKind::WithBarrier { projected } => {
                    // We are crossing a barrier. Only pass through if `name`
                    // is in the projected list.
                    let passes = projected
                        .iter()
                        .any(|(proj_name, _)| proj_name.as_str() == name);
                    if !passes {
                        return None;
                    }
                    // Continue with the parent (which holds the projected alias
                    // binding or the original binding prior to the WITH).
                    match node.parent {
                        Some(p) => current = p,
                        None => return None,
                    }
                }
                _ => match node.parent {
                    Some(p) => current = p,
                    None => return None,
                },
            }
        }
    }

    /// Immutable access to a scope node.
    pub fn scope(&self, id: ScopeId) -> &ScopeNode {
        &self.nodes[id.0 as usize]
    }

    /// Total number of scopes in the graph.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns `true` if the graph is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Iterate over all scopes in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &ScopeNode> {
        self.nodes.iter()
    }
}

// ---------------------------------------------------------------------------
// ResolvedNames
// ---------------------------------------------------------------------------

/// The kind of a binding as recorded in `ResolvedNames`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingKind {
    /// Node pattern variable.
    Node,
    /// Relationship pattern variable.
    Relationship,
    /// Path binder.
    Path,
    /// UNWIND / WITH / CALL YIELD value variable.
    Value,
}

impl From<VarKind> for BindingKind {
    fn from(k: VarKind) -> Self {
        match k {
            VarKind::Node => Self::Node,
            VarKind::Relationship => Self::Relationship,
            VarKind::Path => Self::Path,
            VarKind::Value => Self::Value,
        }
    }
}

/// One entry in `ResolvedNames`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedBinding {
    /// The `VarId` at the binding site (definition).
    pub defining_var: VarId,
    /// Variable kind at the definition site.
    pub kind: BindingKind,
    /// `true` if this resolution crossed a WITH barrier via projection.
    pub via_projection: bool,
}

/// Outcome for a single use-site variable reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Successfully resolved to a binding.
    Resolved(ResolvedBinding),
    /// Not found in any enclosing scope — an unresolved reference.
    Unresolved,
}

/// Map from use-site `VarId` to its `Resolution` (spec §6.2).
///
/// Keys are the `VarId`s that appear as `Expr::Var(id)` (resolved) or
/// that were emitted as `Expr::Unresolved(name)` (unresolved).  The
/// table is populated by the resolver pass in `cypher-sema`.
///
/// Ordering is by `VarId` insertion order (deterministic, per §8/AGENTS.md).
#[derive(Debug, Clone, Default)]
pub struct ResolvedNames {
    map: IndexMap<VarId, Resolution>,
}

impl ResolvedNames {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a resolution for `use_var`.
    pub fn insert(&mut self, use_var: VarId, resolution: Resolution) {
        self.map.insert(use_var, resolution);
    }

    /// Query the resolution of a use-site variable.
    pub fn get(&self, use_var: VarId) -> Option<&Resolution> {
        self.map.get(&use_var)
    }

    /// Number of entries in the table.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns `true` if the table is empty.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Iterate over `(use_var, resolution)` pairs in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (VarId, &Resolution)> {
        self.map.iter().map(|(k, v)| (*k, v))
    }

    /// How many references are unresolved.
    pub fn unresolved_count(&self) -> usize {
        self.map
            .values()
            .filter(|r| matches!(r, Resolution::Unresolved))
            .count()
    }
}
