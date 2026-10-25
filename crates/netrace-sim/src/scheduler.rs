use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashSet};
use std::fmt;
use std::time::Duration;

use crate::SimTime;

/// Handle to a scheduled event, used to cancel it before it fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventId(u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleError {
    /// The requested time is earlier than the scheduler's current time.
    InThePast { requested: SimTime, now: SimTime },
}

impl std::error::Error for ScheduleError {}

impl fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScheduleError::InThePast { requested, now } => {
                write!(
                    f,
                    "cannot schedule at {requested}, clock is already at {now}"
                )
            }
        }
    }
}

struct Entry<E> {
    at: SimTime,
    seq: u64,
    event: E,
}

// Ordered by (time, insertion sequence) only, so events at the same instant
// fire in the order they were scheduled. The payload never affects ordering.
impl<E> PartialEq for Entry<E> {
    fn eq(&self, other: &Self) -> bool {
        self.at == other.at && self.seq == other.seq
    }
}

impl<E> Eq for Entry<E> {}

impl<E> PartialOrd for Entry<E> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<E> Ord for Entry<E> {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.at, self.seq).cmp(&(other.at, other.seq))
    }
}

/// A deterministic priority queue of timed events driving the virtual clock.
///
/// Events are delivered in time order; ties are broken by scheduling order.
/// Popping an event advances [`Scheduler::now`] to that event's time.
pub struct Scheduler<E> {
    now: SimTime,
    next_seq: u64,
    queue: BinaryHeap<Reverse<Entry<E>>>,
    /// Sequence numbers of events still due to fire. Cancelling removes an
    /// entry here; its heap slot is discarded lazily when it reaches the top.
    live: HashSet<u64>,
}

impl<E> Default for Scheduler<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E> Scheduler<E> {
    pub fn new() -> Self {
        Scheduler {
            now: SimTime::ZERO,
            next_seq: 0,
            queue: BinaryHeap::new(),
            live: HashSet::new(),
        }
    }

    pub fn now(&self) -> SimTime {
        self.now
    }

    /// Number of events still due to fire.
    pub fn pending(&self) -> usize {
        self.live.len()
    }

    pub fn is_idle(&self) -> bool {
        self.pending() == 0
    }

    pub fn schedule_at(&mut self, at: SimTime, event: E) -> Result<EventId, ScheduleError> {
        if at < self.now {
            return Err(ScheduleError::InThePast {
                requested: at,
                now: self.now,
            });
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.queue.push(Reverse(Entry { at, seq, event }));
        self.live.insert(seq);
        Ok(EventId(seq))
    }

    pub fn schedule_in(&mut self, delay: Duration, event: E) -> EventId {
        let at = self.now + delay;
        self.schedule_at(at, event)
            .expect("now + delay is never in the past")
    }

    /// Cancels a pending event. Returns `false` if it already fired or was cancelled.
    pub fn cancel(&mut self, id: EventId) -> bool {
        self.live.remove(&id.0)
    }

    /// Time of the next live event, if any.
    pub fn peek_time(&mut self) -> Option<SimTime> {
        self.discard_cancelled_head();
        self.queue.peek().map(|Reverse(e)| e.at)
    }

    /// Removes the next event and advances the clock to its time.
    pub fn pop(&mut self) -> Option<(SimTime, E)> {
        self.discard_cancelled_head();
        let Reverse(entry) = self.queue.pop()?;
        self.live.remove(&entry.seq);
        self.now = entry.at;
        Some((entry.at, entry.event))
    }

    /// Pops the next event if it is due at or before `limit`. Otherwise
    /// advances the clock to `limit` and returns `None`.
    pub fn pop_until(&mut self, limit: SimTime) -> Option<(SimTime, E)> {
        match self.peek_time() {
            Some(at) if at <= limit => self.pop(),
            _ => {
                self.now = self.now.max(limit);
                None
            }
        }
    }

    fn discard_cancelled_head(&mut self) {
        while let Some(Reverse(head)) = self.queue.peek() {
            if self.live.contains(&head.seq) {
                break;
            }
            self.queue.pop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(s: &mut Scheduler<&'static str>) -> Vec<(u64, &'static str)> {
        std::iter::from_fn(|| s.pop())
            .map(|(t, e)| (t.as_millis(), e))
            .collect()
    }

    #[test]
    fn delivers_in_time_order() {
        let mut s = Scheduler::new();
        s.schedule_at(SimTime::from_millis(30), "c").unwrap();
        s.schedule_at(SimTime::from_millis(10), "a").unwrap();
        s.schedule_at(SimTime::from_millis(20), "b").unwrap();
        assert_eq!(drain(&mut s), vec![(10, "a"), (20, "b"), (30, "c")]);
    }

    #[test]
    fn same_instant_is_fifo() {
        let mut s = Scheduler::new();
        for name in ["first", "second", "third"] {
            s.schedule_at(SimTime::from_millis(5), name).unwrap();
        }
        assert_eq!(
            drain(&mut s),
            vec![(5, "first"), (5, "second"), (5, "third")]
        );
    }

    #[test]
    fn popping_advances_the_clock() {
        let mut s = Scheduler::new();
        s.schedule_in(Duration::from_millis(40), "x");
        assert_eq!(s.now(), SimTime::ZERO);
        s.pop();
        assert_eq!(s.now(), SimTime::from_millis(40));
        // Relative scheduling is from the new "now".
        s.schedule_in(Duration::from_millis(10), "y");
        assert_eq!(s.pop(), Some((SimTime::from_millis(50), "y")));
    }

    #[test]
    fn rejects_events_in_the_past() {
        let mut s = Scheduler::new();
        s.schedule_at(SimTime::from_millis(10), "x").unwrap();
        s.pop();
        assert_eq!(
            s.schedule_at(SimTime::from_millis(5), "late"),
            Err(ScheduleError::InThePast {
                requested: SimTime::from_millis(5),
                now: SimTime::from_millis(10),
            })
        );
    }

    #[test]
    fn cancelled_events_never_fire() {
        let mut s = Scheduler::new();
        let a = s.schedule_at(SimTime::from_millis(1), "a").unwrap();
        s.schedule_at(SimTime::from_millis(2), "b").unwrap();
        assert!(s.cancel(a));
        assert!(!s.cancel(a), "double cancel reports false");
        assert_eq!(s.pending(), 1);
        assert_eq!(drain(&mut s), vec![(2, "b")]);
    }

    #[test]
    fn cancelling_a_fired_event_is_a_no_op() {
        let mut s = Scheduler::new();
        let a = s.schedule_at(SimTime::from_millis(1), "a").unwrap();
        s.pop();
        assert!(!s.cancel(a));
        assert!(s.is_idle());
    }

    #[test]
    fn pop_until_stops_at_limit_and_advances_clock() {
        let mut s = Scheduler::new();
        s.schedule_at(SimTime::from_millis(10), "a").unwrap();
        s.schedule_at(SimTime::from_millis(50), "b").unwrap();

        assert_eq!(
            s.pop_until(SimTime::from_millis(20)),
            Some((SimTime::from_millis(10), "a"))
        );
        assert_eq!(s.pop_until(SimTime::from_millis(20)), None);
        assert_eq!(s.now(), SimTime::from_millis(20));
        assert_eq!(s.pending(), 1);
    }

    #[test]
    fn pop_until_never_moves_clock_backwards() {
        let mut s: Scheduler<&str> = Scheduler::new();
        s.pop_until(SimTime::from_millis(100));
        s.pop_until(SimTime::from_millis(50));
        assert_eq!(s.now(), SimTime::from_millis(100));
    }
}
