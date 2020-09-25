use std::future::Future;
use std::io;
use std::mem::ManuallyDrop;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::ready;

use crate::{Event, Drive, Ring};

/// A [`Future`] representing an event submitted to io-uring
pub struct Submission<E: Event, D: Drive> {
    ring: Ring<D>,
    event: Option<ManuallyDrop<E>>,
}

impl<E: Event, D: Drive> Submission<E, D> {
    /// Construct a new submission from an event and a driver.
    pub fn new(event: E, driver: D) -> Submission<E, D> {
        Submission {
            ring: Ring::new(driver),
            event: Some(ManuallyDrop::new(event)),
        }
    }

    /// Access the driver this submission is using
    pub fn driver(&self) -> &D {
        self.ring.driver()
    }

    pub fn replace_event(self: Pin<&mut Self>, event: E) {
        let (ring, event_slot) = self.split();
        if let Some(event) = &mut *event_slot {
            let cancellation = unsafe { Event::cancel(event) };
            ring.cancel_pinned(cancellation);
        }
        *event_slot = Some(ManuallyDrop::new(event));
    }

    fn split(self: Pin<&mut Self>) -> (Pin<&mut Ring<D>>, &mut Option<ManuallyDrop<E>>) {
        unsafe {
            let this = Pin::get_unchecked_mut(self);
            (Pin::new_unchecked(&mut this.ring), &mut this.event)
        }
    }
}

impl<E, D> Future for Submission<E, D> where
    E: Event,
    D: Drive,
{
    type Output = (E, io::Result<u32>);

    fn poll(self: Pin<&mut Self>, ctx: &mut Context<'_>) -> Poll<Self::Output> {
        let (ring, event) = self.split();

        let result = if let Some(event) = event {
            let count = event.sqes_needed();
            ready!(ring.poll(ctx, count, |sqs| unsafe { event.prepare(sqs) }))
        } else {
            panic!("polled Submission after completion")
        };

        Poll::Ready((ManuallyDrop::into_inner(event.take().unwrap()), result))
    }
}


impl<E: Event, D: Drive> Drop for Submission<E, D> {
    fn drop(&mut self) {
        if let Some(event) = &mut self.event {
            let cancellation = unsafe { Event::cancel(event) };
            self.ring.cancel(cancellation);
        }
    }
}
