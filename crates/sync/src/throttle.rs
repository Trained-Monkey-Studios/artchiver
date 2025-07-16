use parking_lot::Mutex;
use std::{
    sync::Arc,
    thread::sleep,
    time::{Duration, Instant},
};

#[derive(Debug)]
struct CallingThrottleData {
    nb_call_times_limit: usize,
    expired_time: Duration,
    timestamps: Vec<Instant>,
}

#[derive(Clone, Debug)]
pub struct CallingThrottle {
    lock: Arc<Mutex<CallingThrottleData>>,
}

impl Default for CallingThrottle {
    fn default() -> Self {
        Self::new(10, Duration::from_secs(1))
    }
}

impl CallingThrottle {
    pub fn new(nb_call_times_limit: usize, expired_time: Duration) -> Self {
        Self {
            lock: Arc::new(Mutex::new(CallingThrottleData {
                nb_call_times_limit,
                expired_time,
                timestamps: Vec::new(),
            })),
        }
    }

    pub fn throttle(&self) {
        let mut data = self.lock.lock();
        while data.timestamps.len() == data.nb_call_times_limit {
            let now = Instant::now();
            let timeout = data.expired_time;
            data.timestamps.retain(|&x| now.duration_since(x) < timeout);
            if data.timestamps.len() >= data.nb_call_times_limit {
                let time_to_sleep = data.timestamps[0] + data.expired_time - now;
                sleep(time_to_sleep);
            }
        }
        data.timestamps.push(Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_throttle() {
        let throttle = CallingThrottle::new(30, Duration::from_secs(1));
        let start = Instant::now();
        for _ in 0..100 {
            throttle.throttle();
        }
        assert!(start.elapsed() > Duration::from_secs(3));
    }

    #[test]
    fn test_throttle_window() {
        let throttle = CallingThrottle::new(1, Duration::from_secs(2));
        let start = Instant::now();
        for _ in 0..3 {
            throttle.throttle();
            /*
            +0 -> #1
            sleep(2)
            +2 -> #2
            sleep(2)
            +4 -> #3
            sleep(2)
            +6 -> #4
            ...
             */
        }
        assert!(dbg!(start.elapsed()) > Duration::from_secs(3));
    }
}
