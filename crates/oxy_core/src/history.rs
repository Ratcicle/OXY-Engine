//! Bounded snapshot commands. Begin once on pointer-down, commit on pointer-up.
#[derive(Clone, Debug)]
pub struct History<T> {
    past: Vec<(String, T)>,
    future: Vec<(String, T)>,
    current: T,
    pending: Option<(String, T)>,
    capacity: usize,
}
impl<T: Clone + PartialEq> History<T> {
    pub fn new(initial: T) -> Self {
        Self {
            past: Vec::new(),
            future: Vec::new(),
            current: initial,
            pending: None,
            capacity: 100,
        }
    }
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity.max(1);
        self.trim();
    }
    pub fn current(&self) -> &T {
        &self.current
    }
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }
    pub fn undo_label(&self) -> Option<&str> {
        self.past.last().map(|(label, _)| label.as_str())
    }
    pub fn redo_label(&self) -> Option<&str> {
        self.future.last().map(|(label, _)| label.as_str())
    }
    pub fn record(&mut self, label: impl Into<String>, after: T) -> bool {
        if self.pending.is_some() {
            return false;
        }
        if self.current == after {
            return false;
        }
        let before = std::mem::replace(&mut self.current, after);
        self.past.push((label.into(), before));
        self.future.clear();
        self.trim();
        true
    }
    pub fn begin(&mut self, label: impl Into<String>) {
        if self.pending.is_none() {
            self.pending = Some((label.into(), self.current.clone()));
        }
    }
    pub fn commit(&mut self, after: T) -> bool {
        let Some((label, before)) = self.pending.take() else {
            return self.record("Editar", after);
        };
        self.current = before;
        self.record(label, after)
    }
    pub fn cancel(&mut self) -> Option<T> {
        self.pending.take().map(|(_, before)| {
            self.current = before.clone();
            before
        })
    }
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn undo(&mut self) -> Option<T> {
        if self.pending.is_some() {
            return self.cancel();
        }
        let (label, previous) = self.past.pop()?;
        self.future
            .push((label, std::mem::replace(&mut self.current, previous)));
        Some(self.current.clone())
    }
    pub fn redo(&mut self) -> Option<T> {
        if self.pending.is_some() {
            return None;
        }
        let (label, next) = self.future.pop()?;
        self.past
            .push((label, std::mem::replace(&mut self.current, next)));
        Some(self.current.clone())
    }
    pub fn reset(&mut self, value: T) {
        self.current = value;
        self.past.clear();
        self.future.clear();
        self.pending = None;
    }
    fn trim(&mut self) {
        if self.past.len() > self.capacity {
            self.past.drain(..self.past.len() - self.capacity);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn undo_redo_and_branch() {
        let mut h = History::new(0);
        h.record("a", 1);
        h.record("b", 2);
        assert_eq!(h.undo(), Some(1));
        assert_eq!(h.redo(), Some(2));
        h.undo();
        h.record("c", 3);
        assert!(!h.can_redo());
        assert_eq!(h.undo(), Some(1));
    }
    #[test]
    fn gesture_is_one_command() {
        let mut h = History::new(vec![0]);
        h.begin("Traço");
        assert!(!h.record("intermediário", vec![1]));
        h.commit(vec![2, 3]);
        assert_eq!(h.undo(), Some(vec![0]));
        assert_eq!(h.undo(), None);
        assert_eq!(h.redo(), Some(vec![2, 3]));
    }
}
