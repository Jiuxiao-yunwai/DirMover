//! FIFO reservations wait asynchronously; only the current job uses a worker.
use crate::error::{AppError, AppResult};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tauri::async_runtime::{Receiver, Sender, channel};

#[derive(Default)]
pub struct MigrationQueue {
    inner: Mutex<QueueState>,
}

#[derive(Default)]
struct QueueState {
    active: bool,
    waiting: VecDeque<Sender<MigrationTurn>>,
}

pub struct MigrationReservation(Receiver<MigrationTurn>);

pub struct MigrationTurn {
    queue: Option<Arc<MigrationQueue>>,
}

impl MigrationQueue {
    pub fn reserve(self: &Arc<Self>) -> AppResult<(MigrationReservation, usize)> {
        let (sender, receiver) = channel(1);
        let mut state = self.inner.lock().map_err(queue_error)?;
        let ahead = state.waiting.len() + usize::from(state.active);
        if state.active {
            state.waiting.push_back(sender);
        } else {
            state.active = true;
            drop(state);
            // The receiver is still owned here, so this channel cannot be closed.
            sender
                .try_send(MigrationTurn {
                    queue: Some(Arc::clone(self)),
                })
                .map_err(|_| AppError::Task("无法分配迁移执行权。".to_string()))?;
        }
        Ok((MigrationReservation(receiver), ahead))
    }

    pub async fn wait_turn(
        &self,
        mut reservation: MigrationReservation,
    ) -> AppResult<MigrationTurn> {
        reservation
            .0
            .recv()
            .await
            .ok_or_else(|| AppError::Task("迁移队列已关闭。".to_string()))
    }

    fn release(self: &Arc<Self>) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        while let Some(sender) = state.waiting.pop_front() {
            match sender.try_send(MigrationTurn {
                queue: Some(Arc::clone(self)),
            }) {
                Ok(()) => return,
                Err(error) => {
                    // A dropped waiter must not stall the queue or recursively
                    // release while its mutex is held.
                    error.into_inner().queue.take();
                }
            }
        }
        state.active = false;
    }
}

impl Drop for MigrationTurn {
    fn drop(&mut self) {
        if let Some(queue) = self.queue.take() {
            queue.release();
        }
    }
}

fn queue_error<T>(_: T) -> AppError {
    AppError::Task("迁移队列状态已损坏，请重新启动应用。".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservations_run_in_fifo_order_without_a_thread_per_waiter() {
        tauri::async_runtime::block_on(async {
            let queue = Arc::new(MigrationQueue::default());
            let mut reservations = Vec::new();
            for ahead in 0..160 {
                let (reservation, actual_ahead) = queue.reserve().unwrap();
                assert_eq!(ahead, actual_ahead);
                reservations.push(reservation);
            }
            let order = Arc::new(Mutex::new(Vec::new()));
            let mut jobs = Vec::new();
            // Poll them in reverse reservation order to exercise FIFO handoff.
            for (index, reservation) in reservations.into_iter().enumerate().rev() {
                let queue = Arc::clone(&queue);
                let order = Arc::clone(&order);
                jobs.push(tauri::async_runtime::spawn(async move {
                    let _turn = queue.wait_turn(reservation).await.unwrap();
                    order.lock().unwrap().push(index);
                }));
            }
            for job in jobs {
                job.await.unwrap();
            }
            assert_eq!(*order.lock().unwrap(), (0..160).collect::<Vec<_>>());
            assert_eq!(queue.reserve().unwrap().1, 0);
        });
    }

    #[test]
    fn abandoned_reservations_release_or_skip_their_turn() {
        tauri::async_runtime::block_on(async {
            let queue = Arc::new(MigrationQueue::default());
            let (first, _) = queue.reserve().unwrap();
            let (abandoned, _) = queue.reserve().unwrap();
            let (last, _) = queue.reserve().unwrap();
            drop(abandoned);
            drop(first); // Its ready permit is in the channel and must release.
            drop(queue.wait_turn(last).await.unwrap());
            assert_eq!(queue.reserve().unwrap().1, 0);
        });
    }
}
