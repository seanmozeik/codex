//! Bounded lexical environments. Function values refer to stable scope indices.
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
}

impl Bindings {
    pub(super) fn new(values: HashMap<String, Value>) -> Self {
        Self {
            current: ScopeId(0),
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
    pub(super) fn insert(&mut self, name: String, value: Value) {
        self.scopes[self.current.0].values.insert(name, value);
    }
    pub(super) fn assign(&mut self, name: &str, value: Value) {
        let mut scope = Some(self.current);
        while let Some(index) = scope {
            let frame = &mut self.scopes[index.0];
            if frame.values.contains_key(name) {
                frame.values.insert(name.into(), value);
                return;
            }
            scope = frame.parent;
        }
        self.clear();
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
        // Do not recycle scope IDs referenced by closures from other branches.
        for (index, scope) in saved.scopes.iter().enumerate() {
            self.scopes[index] = scope.clone();
        }
        self.current = saved.current;
    }
    pub(super) fn join(&mut self, other: &Self) {
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
