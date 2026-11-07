//! Reines Datenmodell – kennt keinen Browser und kein Leptos.
//! Dadurch lässt es sich mit `cargo test` direkt testen.

#[derive(Clone, Debug, PartialEq)]
pub struct Todo {
    pub id: u32,
    pub text: String,
    pub done: bool,
}

#[derive(Clone, Debug, Default)]
pub struct TodoList {
    next_id: u32,
    items: Vec<Todo>,
}

impl TodoList {
    pub fn from_texts<I, S>(texts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut list = Self::default();
        for text in texts {
            list.add(text.as_ref());
        }
        list
    }

    /// Fügt einen Eintrag hinzu. Leere Texte werden ignoriert (`None`).
    pub fn add(&mut self, text: &str) -> Option<u32> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(Todo {
            id,
            text: text.to_owned(),
            done: false,
        });
        Some(id)
    }

    pub fn toggle(&mut self, id: u32) {
        if let Some(todo) = self.items.iter_mut().find(|t| t.id == id) {
            todo.done = !todo.done;
        }
    }

    pub fn remove(&mut self, id: u32) {
        self.items.retain(|t| t.id != id);
    }

    pub fn items(&self) -> &[Todo] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn remaining(&self) -> usize {
        self.items.iter().filter(|t| !t.done).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_ignores_blank_text() {
        let mut list = TodoList::default();
        assert_eq!(list.add("   "), None);
        assert!(list.is_empty());
    }

    #[test]
    fn add_toggle_remove() {
        let mut list = TodoList::default();
        let a = list.add("Rust lernen").unwrap();
        let b = list.add("  WASM bauen  ").unwrap();
        assert_ne!(a, b);
        assert_eq!(list.items()[1].text, "WASM bauen");
        assert_eq!(list.remaining(), 2);

        list.toggle(a);
        assert_eq!(list.remaining(), 1);

        list.remove(a);
        assert_eq!(list.len(), 1);
        assert_eq!(list.items()[0].id, b);
    }

    #[test]
    fn ids_are_not_reused_after_remove() {
        let mut list = TodoList::from_texts(["a", "b"]);
        list.remove(1);
        let c = list.add("c").unwrap();
        assert_eq!(c, 2);
    }
}
