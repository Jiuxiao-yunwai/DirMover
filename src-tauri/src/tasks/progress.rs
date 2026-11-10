use crate::models::ScanProgress;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct ProgressThrottle {
    last: Option<(ScanProgress, Instant)>,
}

impl ProgressThrottle {
    pub fn allows(&mut self, update: &ScanProgress, now: Instant) -> bool {
        let send = self.last.as_ref().is_none_or(|(last, sent)| {
            update.phase != last.phase
                || (update.percentage == Some(100) && update != last)
                || (update != last && now.duration_since(*sent) >= Duration::from_millis(80))
        });
        if send {
            self.last = Some((update.clone(), now));
        }
        send
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phase_changes_and_completion_are_never_lost_to_throttling() {
        let now = Instant::now();
        let mut throttle = ProgressThrottle::default();
        let measure = ScanProgress {
            percentage: None,
            phase: "measure".into(),
            message: "scan".into(),
        };
        assert!(throttle.allows(&measure, now));
        let copy = ScanProgress {
            phase: "copy".into(),
            message: "copy".into(),
            ..measure
        };
        assert!(throttle.allows(&copy, now + Duration::from_millis(2)));
        let update = ScanProgress {
            message: "more directories".into(),
            ..copy.clone()
        };
        assert!(!throttle.allows(&update, now + Duration::from_millis(3)));
        assert!(throttle.allows(&update, now + Duration::from_millis(100)));
        assert!(!throttle.allows(&update, now + Duration::from_millis(200)));
        assert!(throttle.allows(
            &ScanProgress {
                percentage: Some(100),
                ..update
            },
            now + Duration::from_millis(201)
        ));
    }
}
