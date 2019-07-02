use hashbrown::HashMap;
use tracing_core::span::Id;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    cell::{RefCell, Cell},
};
use parking_lot::{ReentrantMutex};
use crossbeam_utils::sync::ShardedLock;

pub struct Registry<T> {
    shards: ShardedLock<HashMap<Thread, Shard<T>>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct Thread {
    id: usize,
}

struct Shard<T> {
    spans: ReentrantMutex<RefCell<HashMap<Id, T>>>,
}

impl<T: 'static> Registry<T> {
    fn with_shard<I>(&self, f: impl FnMut(Shard<T>) -> I) -> I {
        // fast path --- the shard already exists
        let thread = Thread::current();
        let fast = {
            let shards = self.shards.read().unwrap();
            shards.get(&thread).map(&mut f)
        };
        if let Some(r) = fast {
            return r
        } else {
            // slow path --- need to insert a shard.
            let mut shards = self.shards.write().unwrap();
            shards.insert(thread, Shard::new());
            shards.get(&thread).map(&mut f).unwrap()
        }
    }
}

impl<T> Shard<T> {
    fn new() -> Self {
        Self {
            spans: ReentrantMutex::new(RefCell::new(HashMap::new()))
        }
    }
}

impl Thread {
    fn current() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        thread_local! {
            static MY_ID: Cell<Option<usize>> = Cell::new(None);
        }
        MY_ID.with(|my_id| if let Some(id) = my_id.get() {
            Thread {
                id
            }
        } else {
            let id = NEXT.fetch_add(1, Ordering::SeqCst);
            my_id.set(Some(id));
            Thread {
                id
            }
        })
    }
}
