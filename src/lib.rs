use std::any::Any;
use std::cell::Cell;
use std::marker::PhantomData;
use std::rc::Rc;

/// A token store
///
/// This struct allows you to store various values in a store
/// and access them back using the provided tokens.
pub struct Store {
    values: Vec<Option<(Box<Any>, Rc<Cell<bool>>)>>,
}

/// A token for accessing the store contents
pub struct Token<V> {
    id: usize,
    live: Rc<Cell<bool>>,
    _type: PhantomData<V>,
}

impl<V> Clone for Token<V> {
    fn clone(&self) -> Token<V> {
        Token {
            id: self.id,
            live: self.live.clone(),
            _type: PhantomData,
        }
    }
}

impl Store {
    /// Create a new store
    pub fn new() -> Store {
        Store { values: Vec::new() }
    }

    /// Insert a new value in this store
    ///
    /// Returns a clonable token that you can later use to access this
    /// value.
    pub fn insert<V: Any + 'static>(&mut self, value: V) -> Token<V> {
        let boxed = Box::new(value) as Box<Any>;
        let live = Rc::new(Cell::new(true));
        {
            // artificial scope to make the borrow checker happy
            let empty_slot = self.values
                .iter_mut()
                .enumerate()
                .find(|&(_, ref s)| s.is_none());
            if let Some((id, slot)) = empty_slot {
                *slot = Some((boxed, live.clone()));
                return Token {
                    id: id,
                    live: live,
                    _type: PhantomData,
                };
            }
        }
        self.values.push(Some((boxed, live.clone())));
        Token {
            id: self.values.len() - 1,
            live: live,
            _type: PhantomData,
        }
    }

    /// Access value previously inserted in this store
    ///
    /// Panics if the provided token corresponds to a value that was removed.
    pub fn get<V: Any + 'static>(&self, token: &Token<V>) -> &V {
        if !token.live.get() {
            panic!("Attempted to access a state value that was already removed!");
        }
        self.values[token.id]
            .as_ref()
            .and_then(|t| t.0.downcast_ref::<V>())
            .unwrap()
    }

    /// Mutably access value previously inserted in this store
    ///
    /// Panics if the provided token corresponds to a value that was removed.
    pub fn get_mut<V: Any + 'static>(&mut self, token: &Token<V>) -> &mut V {
        if !token.live.get() {
            panic!("Attempted to access a state value that was already removed!");
        }
        self.values[token.id]
            .as_mut()
            .and_then(|t| t.0.downcast_mut::<V>())
            .unwrap()
    }

    /// Remove a value previously inserted in this store
    ///
    /// Panics if the provided token corresponds to a value that was already
    /// removed.
    pub fn remove<V: Any + 'static>(&mut self, token: Token<V>) -> V {
        if !token.live.get() {
            panic!("Attempted to remove a state value that was already removed!");
        }
        let (boxed, live) = self.values[token.id].take().unwrap();
        live.set(false);
        *boxed.downcast().unwrap()
    }

    pub fn with_value<V: Any + 'static, T, F>(&mut self, token: &Token<V>, f: F) -> T
    where
        F: FnOnce(&mut StoreProxy, &mut V) -> T,
    {
        self.as_proxy().with_value(token, f)
    }


    pub fn as_proxy<'a>(&'a mut self) -> StoreProxy<'a> {
        StoreProxy {
            store: self,
            borrowed: Vec::new(),
        }
    }
}

pub struct StoreProxy<'store> {
    store: &'store mut Store,
    borrowed: Vec<usize>,
}

impl<'store> StoreProxy<'store> {
    /// Insert a new value in the proxified store
    ///
    /// Returns a clonable token that you can later use to access this
    /// value.
    pub fn insert<V: Any + 'static>(&mut self, value: V) -> Token<V> {
        self.store.insert(value)
    }

    /// Access value previously inserted in the proxified store
    ///
    /// Panics if the provided token corresponds to a value that was removed, or
    /// if this value is already borrowed.
    pub fn get<V: Any + 'static>(&self, token: &Token<V>) -> &V {
        if self.borrowed.contains(&token.id) {
            panic!("Attempted to borrow twice the same value from the Store!");
        }
        self.store.get(token)
    }

    /// Mutably access value previously inserted in the proxified store
    ///
    /// Panics if the provided token corresponds to a value that was removed, or
    /// if this value is already borrowed.
    pub fn get_mut<V: Any + 'static>(&mut self, token: &Token<V>) -> &mut V {
        if self.borrowed.contains(&token.id) {
            panic!("Attempted to borrow twice the same value from the Store!");
        }
        self.store.get_mut(token)
    }

    /// Remove a value previously inserted in the proxified store
    ///
    /// Panics if the provided token corresponds to a value that was already
    /// removed.
    pub fn remove<V: Any + 'static>(&mut self, token: Token<V>) -> V {
        if self.borrowed.contains(&token.id) {
            panic!("Attempted to remove a value from the Store while it was borrowed!");
        }
        self.store.remove(token)
    }

    pub fn with_value<V: Any + 'static, T, F>(&mut self, token: &Token<V>, f: F) -> T
    where
        F: FnOnce(&mut StoreProxy, &mut V) -> T,
    {
        if self.borrowed.contains(&token.id) {
            panic!("Attempted to borrow twice the same value from the Store!");
        }
        let value_ptr = { self.store.get_mut(token) as *mut V };
        let value = unsafe { &mut *value_ptr };
        let mut deeper_proxy = StoreProxy {
            store: &mut *self.store,
            borrowed: {
                let mut my_borrowed = self.borrowed.clone();
                my_borrowed.push(token.id);
                my_borrowed
            },
        };
        f(&mut deeper_proxy, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn insert_and_retrieve() {
        let mut store = Store::new();
        let token1 = store.insert(42);
        let token2 = store.insert("I like trains".to_owned());
        assert_eq!(*store.get(&token1), 42);
        assert_eq!(store.get(&token2), "I like trains");
    }

    #[test]
    fn mutate() {
        let mut store = Store::new();
        let token = store.insert(42);
        {
            let v = store.get_mut(&token);
            *v += 5;
        }
        assert_eq!(*store.get(&token), 47);
    }

    #[test]
    #[should_panic]
    fn no_access_removed() {
        let mut store = Store::new();
        let token = store.insert(42);
        let token2 = token.clone();
        store.remove(token2);
        let _v = store.get(&token);
    }

    #[test]
    #[should_panic]
    fn no_mut_access_removed() {
        let mut store = Store::new();
        let token = store.insert(42);
        let token2 = token.clone();
        store.remove(token2);
        let _v = store.get_mut(&token);
    }

    #[test]
    #[should_panic]
    fn no_double_remove() {
        let mut store = Store::new();
        let token = store.insert(42);
        let token2 = token.clone();
        store.remove(token2);
        store.remove(token);
    }


    #[test]
    fn place_reuse() {
        let mut store = Store::new();
        let token = store.insert(42);
        store.remove(token);
        let token = store.insert("I like trains");
        assert_eq!(store.values.len(), 1);
        assert_eq!(*store.get(&token), "I like trains");
    }

    #[test]
    fn with_value_manipulate() {
        let mut store = Store::new();
        let token1 = store.insert("I like trains".to_owned());
        let token2 = store.insert(42);
        let len = store.with_value(&token1, |proxy, value1| {
            *proxy.get_mut(&token2) += 10;
            let token3 = proxy.with_value(&token2, |proxy, value2| {
                *value2 *= 2;
                proxy.insert(*value2 as f32 + 0.5)
            });
            let number = proxy.remove(token2);
            value1.push_str(&format!(": {} = {}", number, proxy.get(&token3)));
            value1.len()
        });
        assert_eq!(len, 26);
        assert_eq!(store.get(&token1), "I like trains: 104 = 104.5");
    }

    #[test]
    #[should_panic]
    fn no_double_with_value() {
        let mut store = Store::new();
        let token = store.insert(42);
        store.with_value(&token, |proxy, _| {
            proxy.with_value(&token, |_, _| {
            });
        });
    }

    #[test]
    #[should_panic]
    fn no_alias_get_and_with_value() {
        let mut store = Store::new();
        let token = store.insert(42);
        store.with_value(&token, |proxy, _| {
            let _v = proxy.get(&token);
        });
    }

    #[test]
    #[should_panic]
    fn no_alias_get_mut_and_with_value() {
        let mut store = Store::new();
        let token = store.insert(42);
        store.with_value(&token, |proxy, _| {
            let _v = proxy.get_mut(&token);
        });
    }

    #[test]
    #[should_panic]
    fn no_alias_remove_and_with_value() {
        let mut store = Store::new();
        let token = store.insert(42);
        store.with_value(&token, |proxy, _| {
            let _v = proxy.remove(token.clone());
        });
    }

}
