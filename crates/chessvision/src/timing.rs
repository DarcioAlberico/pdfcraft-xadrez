//! The one rule of this crate's timing tests (the `perf_` tests, run `--ignored` in release by
//! the chess gate's stage 4).
//!
//! The gate runs them while other builds load the machine. One run then took up to 1.5 times
//! what it takes on an idle machine, and even the best of three up to 1.2 times. So a timing
//! test times three runs, keeps the best, and fails past **twice** the budget its plan card
//! gives, the same margin the gate gives the timings it has a baseline for (`perf-baseline.json`).
//! What these tests guard against is a step back by an order of magnitude (a quadratic walk
//! coming back); a slowdown of less than twice is the baseline's to catch.

/// `f` run three times: the best time in milliseconds, the three times, and the last result.
pub(crate) fn best_of_three<T>(mut f: impl FnMut() -> T) -> (f64, [f64; 3], T) {
    let mut timed = || {
        let t = std::time::Instant::now();
        let out = f();
        (t.elapsed().as_secs_f64() * 1000.0, out)
    };
    let (a, _) = timed();
    let (b, _) = timed();
    let (c, last) = timed();
    let runs = [a, b, c];
    (a.min(b).min(c), runs, last)
}

/// Whether `best` (milliseconds) is within twice `budget_ms`, the card's budget.
pub(crate) fn within(best: f64, budget_ms: f64) -> bool {
    best < 2.0 * budget_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_best_of_three_runs_and_the_last_result() {
        let mut n = 0;
        let (best, runs, last) = best_of_three(|| {
            n += 1;
            n
        });
        assert_eq!(last, 3, "three runs, the last one's result");
        assert!(runs.iter().all(|r| *r >= 0.0) && best <= runs[0] && best <= runs[1] && best <= runs[2]);
        assert!(within(1999.0, 1000.0) && !within(2000.0, 1000.0));
    }
}
