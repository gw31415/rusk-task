use std::{
    borrow::Borrow,
    hash::Hash,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use hashbrown::{HashMap, HashSet, hash_map::EntryRef};

/// Node of a tree
pub struct TreeNode<K: Hash + Eq + Clone, T> {
    _key: PhantomData<fn() -> K>,
    /// Inner-Item of the node
    pub item: T,
    /// Children of the node
    pub children: Vec<Rc<TreeNode<K, T>>>,
}

/// Error of TreeNode
#[derive(Debug, thiserror::Error)]
pub enum TreeNodeCreationError<K: Hash + Eq + Clone> {
    /// Item not found
    #[error("Item named {0:?} not found")]
    ItemNotFound(K),
    /// Circular dependency found
    #[error("Circular dependency found around {0:?}")]
    CircularDependency(K),
}

/// To manage parents of a node. When the manager is dropped, it removes the parent from the set.
struct ParentsManager<'a, K: Hash + Eq + Clone>(&'a mut HashSet<K>, &'a K);

impl<'a, K: Hash + Eq + Clone> ParentsManager<'a, K> {
    fn new(parents: &'a mut HashSet<K>, name: &'a K) -> Self {
        parents.insert(name.clone());
        Self(parents, name)
    }
}

impl<K: Hash + Eq + Clone> Deref for ParentsManager<'_, K> {
    type Target = HashSet<K>;
    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<K: Hash + Eq + Clone> DerefMut for ParentsManager<'_, K> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
    }
}

impl<K: Hash + Eq + Clone> Drop for ParentsManager<'_, K> {
    fn drop(&mut self) {
        self.0.remove(self.1);
    }
}

impl<K: Hash + Eq + Clone, D: DigraphItem<K>> TreeNode<K, D> {
    /// Create trees from a directed graph.
    pub fn new_vec(
        hashmap: HashMap<K, D>,
        targets: impl IntoIterator<Item: Borrow<K>>,
    ) -> Result<Vec<Self>, TreeNodeCreationError<K>> {
        enum RawOrNode<K: Hash + Eq + Clone, D: DigraphItem<K>> {
            Raw(D),
            Node(Rc<TreeNode<K, D>>),
        }
        fn convert<K: Hash + Eq + Clone, D: DigraphItem<K>>(
            name: &K,
            raw: D,
            list: &mut HashMap<K, RawOrNode<K, D>>,
            parents: &mut HashSet<K>,
        ) -> Result<TreeNode<K, D>, TreeNodeCreationError<K>> {
            let mut parents = ParentsManager::new(parents, name);

            let mut children = vec![];
            for dep_name in raw.children().iter() {
                if parents.contains(dep_name) {
                    return Err(TreeNodeCreationError::CircularDependency(dep_name.clone()));
                }

                match list.entry_ref(dep_name) {
                    EntryRef::Vacant(_) => {
                        return Err(TreeNodeCreationError::ItemNotFound(dep_name.clone()));
                    }
                    EntryRef::Occupied(occupied) => match occupied.remove() {
                        RawOrNode::Raw(dep_item) => {
                            let node = Rc::new(convert(dep_name, dep_item, list, &mut parents)?);
                            list.insert(dep_name.clone(), RawOrNode::Node(node.clone()));
                            children.push(node);
                        }
                        RawOrNode::Node(dep_node) => {
                            list.insert(dep_name.clone(), RawOrNode::Node(dep_node.clone()));
                            children.push(dep_node);
                        }
                    },
                }
            }
            Ok(TreeNode::<K, D> {
                _key: PhantomData,
                item: raw,
                children,
            })
        }

        let mut roots = vec![];
        let mut hashmap = hashmap
            .into_iter()
            .map(|(k, v)| (k, RawOrNode::Raw(v)))
            .collect::<HashMap<_, _>>();
        for label in targets {
            let label = label.borrow();
            let Some(item) = hashmap.remove(label) else {
                return Err(TreeNodeCreationError::ItemNotFound(label.clone()));
            };
            if let RawOrNode::Raw(raw) = item {
                let node = convert(label, raw, &mut hashmap, &mut HashSet::new())?;
                roots.push(node);
            }
        }
        Ok(roots)
    }
}

/// Vertex of a directed graph
pub trait DigraphItem<K: Hash + Eq + Clone> {
    /// Get children of the vertex
    fn children(&self) -> impl Deref<Target = [K]>;
}
