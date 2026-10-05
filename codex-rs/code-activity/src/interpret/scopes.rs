//! Bounded lexical environments. Function values refer to stable scope indices.
use crate::value::MAX_VALUE_BYTES;
use crate::value::Value;
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub(super) struct ScopeId(usize);

#[derive(Clone)]
struct Scope {
    parent: Option<ScopeId>,
    values: HashMap<String, Value>,
}

#[derive(Clone)]
pub(super) struct Bindings {
    current: ScopeId,
    scopes: Vec<Scope>,
    exhausted: bool,
}

impl Bindings {
    pub(super) fn new(values: HashMap<String, Value>) -> Self {
        Self {
            current: ScopeId(0),
            exhausted: false,
            scopes: vec![Scope {
                parent: None,
                values,
            }],
        }
    }
    pub(super) const fn current(&self) -> ScopeId {
        self.current
    }
    pub(super) const fn set_current(&mut self, current: ScopeId) {
        self.current = current;
    }
    pub(super) fn enter(&mut self, parent: ScopeId) -> bool {
        if self.scopes.len() >= 256 {
            return false;
        }
        self.current = ScopeId(self.scopes.len());
        self.scopes.push(Scope {
            parent: Some(parent),
            values: HashMap::new(),
        });
        true
    }
    pub(super) fn len(&self) -> usize {
        self.scopes.iter().map(|scope| scope.values.len()).sum()
    }
    pub(super) fn get(&self, name: &str) -> Option<&Value> {
        if self.exhausted {
            return None;
        }
        let mut scope = Some(self.current);
        while let Some(index) = scope {
            let frame = self.scopes.get(index.0)?;
            if let Some(value) = frame.values.get(name) {
                return Some(value);
            }
            scope = frame.parent;
        }
        None
    }
    pub(super) fn local(&self, name: &str) -> bool {
        self.scopes[self.current.0].values.contains_key(name)
    }
    pub(super) fn insert(&mut self, name: String, value: Value) -> bool {
        if self.exhausted
            || name.len() > MAX_VALUE_BYTES
            || !value.within_budget()
            || !self.local(&name) && self.len() >= 256
        {
            // A dropped local declaration must not reveal an enclosing name.
            // Stop resolving this environment rather than growing shadow slots.
            self.exhausted = true;
            self.clear();
            return false;
        }
        self.scopes[self.current.0].values.insert(name, value);
        true
    }
    pub(super) fn assign(&mut self, name: &str, value: Value) -> bool {
        if self.exhausted || !value.within_budget() {
            self.exhausted = true;
            self.clear();
            return false;
        }
        let mut scope = Some(self.current);
        while let Some(index) = scope {
            let frame = &mut self.scopes[index.0];
            if frame.values.contains_key(name) {
                frame.values.insert(name.into(), value);
                return true;
            }
            scope = frame.parent;
        }
        self.clear();
        true
    }
    pub(super) fn clear(&mut self) {
        // Invalidate retained closures as well as the caller: unknown code may
        // mutate any reachable environment. Keep names to preserve shadowing.
        for scope in &mut self.scopes {
            for value in scope.values.values_mut() {
                *value = Value::Unknown;
            }
        }
    }
    pub(super) fn invalidate_changes(&mut self, before: &Self) {
        // An async body does not establish when writes to enclosing scopes
        // become visible. Preserve fresh invocation frames for awaited closures.
        for (index, previous) in before.scopes.iter().enumerate() {
            for (name, value) in &mut self.scopes[index].values {
                if previous.values.get(name) != Some(value) {
                    *value = Value::Unknown;
                }
            }
        }
    }
    pub(super) fn restore(&mut self, saved: &Self) {
        self.exhausted |= saved.exhausted;
        // Do not recycle scope IDs referenced by closures from other branches.
        for (index, scope) in saved.scopes.iter().enumerate() {
            self.scopes[index] = scope.clone();
        }
        self.current = saved.current;
    }
    pub(super) fn join(&mut self, other: &Self) {
        self.exhausted |= other.exhausted;
        for (index, scope) in self.scopes.iter_mut().enumerate() {
            for (name, value) in &mut scope.values {
                if other.scopes.get(index).and_then(|s| s.values.get(name)) != Some(value) {
                    *value = Value::Unknown;
                }
            }
        }
        self.scopes
            .extend(other.scopes.iter().skip(self.scopes.len()).cloned());
    }
}
