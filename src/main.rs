use std::future::Future;
use std::task::Waker;
use std::task::RawWaker;
use std::task::RawWakerVTable;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::sync::Mutex;
use std::sync::Condvar;
use std::sync::MutexGuard;
use std::task::Wake;
use std::sync::Arc;
use std::time::Duration;
use std::cell::RefCell;
use std::collections::VecDeque;
use scoped_tls::scoped_thread_local;
use futures::future::BoxFuture;
use async_std::task::spawn;



struct Demo;

impl Future for Demo {
    type Output = ();
    fn poll(self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        println!("hello world!");
        std::task::Poll::Ready(())
    }
}

struct Signal {
    state: Mutex<State>,
    cond: Condvar,
}

enum State {
    Empty,
    Waiting,
    Notified,
}

impl State {
    fn new() -> Self {
        State::Empty
    }
}

impl Signal {
    fn new() -> Self {
        let state = Mutex::new(State::new());
        let cond = Condvar::new();

        Signal { state, cond }
    }
    fn wait(&self) {
        let mut state: MutexGuard<'_, State> = self.state.lock().unwrap();
        match *state {
            State::Notified => *state = State::Empty,
            State::Waiting => {
                panic!("multiple wait");
            }
            State::Empty => {
                *state = State::Waiting;
                while let State::Waiting = *state {
                    state = self.cond.wait(state).unwrap();
                }
            }
        }
    }
    
    fn notify(&self) {
        let mut state: MutexGuard<'_, State> = self.state.lock().unwrap();
        match *state {
            State::Notified => {}
            State::Empty => *state = State::Notified,
            State::Waiting => {
                *state = State::Empty;
                self.cond.notify_one();
            }
        }
    }
}


impl Wake for Signal {
    fn wake(self: Arc<Self>) {
        self.notify();
    }
}

scoped_thread_local!(static SIGNAL: Arc<Signal>);
scoped_thread_local!(static RUNNABLE: Mutex<VecDeque<Arc<Task>>>);

struct Task {
    future: RefCell<BoxFuture<'static, ()>>,
    signal: Arc<Signal>,
}
unsafe impl Send for Task {}
unsafe impl Sync for Task {}

impl Wake for Task {
    fn wake(self: Arc<Self>) {
        RUNNABLE.with(|runnable| runnable.lock().unwrap().push_back(self.clone()));
        self.signal.notify();
    }
}
fn block_on<F: Future>(future: F) -> F::Output {
    let mut fut: Pin<&mut F> = std::pin::pin!(future);
    let signal: Arc<Signal> = Arc::new(Signal::new());
    let waker: Waker = Waker::from(signal.clone());
    let mut cx: Context<'_> = Context::from_waker(&waker);
    let runnable = Mutex::new(VecDeque::with_capacity(1024));
    SIGNAL.set(&signal, || {
        RUNNABLE.set(&runnable, || {
            loop {
                if let Poll::Ready(output) = fut.as_mut().poll(&mut cx) {
                    return output;
                }
                while let Some(task) = runnable.lock().unwrap().pop_front() {
                    let waker: Waker = Waker::from(task.clone());
                    let mut cx: Context<'_> = Context::from_waker(&waker);
                    let _ = task.future.borrow_mut().as_mut().poll(&mut cx);
                }
                signal.wait();
            }
        })
    })
    
}

async fn demo() {
    let (tx, rx) = async_channel::bounded(1);
    spawn(demo2(tx));
    println!("hello world!");
    let _ = rx.recv().await;
}

async fn demo2(tx: async_channel::Sender<()>) {
    println!("hello world2!");
    let _ = tx.send(()).await;
}

fn main() {
    block_on(demo());
}
