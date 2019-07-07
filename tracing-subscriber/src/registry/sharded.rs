use hashbrown::HashMap;
use tracing_core::span::Id;
use std::{
    mem,
    thread,
    sync::atomic::{AtomicUsize, Ordering},
    cell::{RefCell, Cell},
};
use parking_lot::{ReentrantMutex, ReentrantMutexGuard, MappedReentrantMutexGuard};
use crossbeam_utils::sync::{ShardedLock, ShardedLockReadGuard};
use owning_ref::OwningHandle;

pub struct Registry<T> {
    shards: ShardedLock<Shards<T>>,
}

#[derive(Copy, Clone, Hash, PartialEq, Eq)]
struct Thread {
    id: usize,
}

struct Shards<T>(HashMap<Thread, Shard<T>>);

struct Shard<T> {
    spans: ReentrantMutex<RefCell<HashMap<Id, T>>>,
}

pub struct Ref<'a, T> {
    inner: OwningHandle<
        ShardedLockReadGuard<'a, Shard<T>>,
        MappedReentrantMutexGuard<'a, &'a mut T>
    >,
}


#[derive(Clone, Debug)]
enum Slot<T> {
    Present(T),
    Stolen(Thread),
}

fn handle_poison<T>(result: Result<T, ()>) -> Option<T> {
    if thread::panicking() {
        result.ok()
    } else {
        Some(result.expect("registry poisoned"))
    }
}

impl<T> Registry<T> {
    fn with_shard<I>(&self, mut f: impl FnOnce(&mut HashMap<Id, T>) -> I) -> Result<I, ()> {
        // fast path --- the shard already exists
        let thread = Thread::current();
        let mut f = Some(f);

        if let Some(r) = self.shards.read().map_err(|_|())?
            .with_shard(&thread, &mut f)
        {
            return Ok(r)
        }
        // slow path --- need to insert a shard.
        self.shards.write().map_err(|_|())?
            .new_shard_for(thread.clone())
            .with_shard(&thread, &mut f).ok_or(())
    }

    pub fn get_span<'a>(&'a self, id: &Id) -> Option<Ref<'a, T>> {
        unimplemented!()
    }

    pub fn with_span<I>(&self, id: &Id, f: impl FnOnce(&mut T) -> I) -> Option<I> {
        let mut f = Some(f);
        let res = self.with_shard(|shard| {
            shard.get_mut(id).and_then(Slot::get_mut).map(|span| {
                let mut f = f.take().expect("called twice!");
                f(span)
            })
        });
        handle_poison(res)?

        // TODO: steal
    }

    pub fn insert(&self, id: Id, span: T) -> &Self {
        let ok = self.with_shard(move |shard| {
            let _ = shard.insert(id, span);
        });
        if !thread::panicking() {
            ok.expect("poisoned");
        }

        self
    }

    pub fn new() -> Self {
        Self {
            shards: ShardedLock::new(Shards(HashMap::new()))
        }
    }
}

impl<T> Shards<T> {
    fn with_shard<I>(
        &self,
        thread: &Thread,
        f: &mut Option<impl FnOnce(&mut HashMap<Id, T>)-> I>,
    ) -> Option<I> {
        let mut lock = self.0.get(thread)?.spans.lock();
        let mut shard = lock.borrow_mut();
        let mut f = f.take()?;
        Some(f(&mut *shard))
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

    fn span<'a>(&'a self, id: &Id) -> Option<ReentrantMutexGuard<'a, &mut T>> {
        let guard = self.spans.lock();
        ReentrantMutexGuard::try_map(
            guard,
            move |spans| spans.get(id).and_then(Slot::get_mut)
        ).ok()
    }

    fn try_steal(&self, id: &Id) -> Option<Slot<T>> {
        let mut lock = self.spans.lock();
        let slot = self.spans.get_mut(id)?;
        mem::replace(slot, Slot::Stolen(Thread::current()))
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

impl<T> Slot<T> {
    fn get_mut(&mut self) -> Option<&mut T> {
        match self {
            Slot::Present(ref mut span) => Some(span),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basically_works() {
        let registry: Registry<usize> = Registry::new();
        registry
            .insert(Id::from_u64(1), 1)
            .insert(Id::from_u64(2), 2);

        assert_eq!(registry.with_span(&Id::from_u64(1), |&mut s| s), Some(1));
        assert_eq!(registry.with_span(&Id::from_u64(2), |&mut s| s), Some(2));

        registry.insert(Id::from_u64(3), 3);

        assert_eq!(registry.with_span(&Id::from_u64(1), |&mut s| s), Some(1));
        assert_eq!(registry.with_span(&Id::from_u64(2), |&mut s| s), Some(2));
        assert_eq!(registry.with_span(&Id::from_u64(3), |&mut s| s), Some(3));
    }
}
