use hashbrown::HashMap;
use tracing_core::span::Id;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    cell::{RefCell, Cell},
};
use parking_lot::{ReentrantMutex};
use crossbeam_utils::sync::ShardedLock;

pub struct Registry<T> {
    shards: ShardedLock<Shards<T>>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct Thread {
    id: usize,
}

struct Shards<T>(HashMap<Thread, Shard<T>>);

struct Shard<T> {
    spans: ReentrantMutex<RefCell<HashMap<Id, T>>>,
}

impl<T: 'static> Registry<T> {
    fn with_shard<I>(&self, mut f: impl FnMut(&mut HashMap<Id, T>) -> I) -> I {
        // fast path --- the shard already exists
        let thread = Thread::current();

        if let Some(r) = self.shards
            .read()
            .with_shard(thread, &mut f)
        {
            return r
        }
        // slow path --- need to insert a shard.
        // TODO(eliza): figure out a good way to propagate poison panics _if_ we are
        // not unwinding?
        self.shards.write()
            .unwrap()
            .new_shard_for(thread)
            .with_shard(thread, &mut f)
            .unwrap()
    }
}

impl<T> Shards<T> {
    fn with_shard<I>(&self, thread: &Thread, f: &mut impl FnMut(&mut HashMap<Id, T>) -> I) -> Option<I> {
        let mut shard = self.0.get(thread).ok()?.lock();
        Some(f(*lock.borrow_mut()))
    }

    fn new_shard_for(&mut self, thread: Thread) -> &mut Self {
        self.0.insert(thread, Shard::new());
        self
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
