//! Адаптивное сглаживание позиции пера (в духе «1€ filter»).
//!
//! Медленные движения и дрожание сенсора сглаживаются, быстрые проходят практически
//! без задержки — поэтому перо остаётся отзывчивым и при этом не дрожит.

/// Скорость (в сырых единицах за отчёт), начиная с которой фильтр полностью открыт.
const OPEN_SPEED: f32 = 6.0;

#[derive(Debug, Clone)]
pub struct PenFilter {
    /// Минимальный коэффициент следования за пером в покое: 1.0 = фильтр выключен.
    alpha_min: f32,
    state: Option<(f32, f32)>,
}

impl PenFilter {
    /// `smooth`: 0.0 — выключено, 0.9 — максимум сглаживания.
    pub fn new(smooth: f32) -> Self {
        PenFilter {
            alpha_min: 1.0 - smooth.clamp(0.0, 0.9),
            state: None,
        }
    }

    pub fn apply(&mut self, x: f32, y: f32) -> (f32, f32) {
        let out = match self.state {
            Some((px, py)) if self.alpha_min < 1.0 => {
                let speed = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
                let open = (speed / OPEN_SPEED).min(1.0);
                let alpha = self.alpha_min + (1.0 - self.alpha_min) * open;
                (px + alpha * (x - px), py + alpha * (y - py))
            }
            _ => (x, y),
        };
        self.state = Some(out);
        out
    }

    /// Начало нового штриха / перо вернулось в зону: без «подтягивания» со старой позиции.
    pub fn reset(&mut self) {
        self.state = None;
    }
}

/// Подавляет фантомные касания: касание начинается только после `need` подряд идущих
/// отчётов выше порога (одиночный всплеск шума не считается), а заканчивается сразу.
#[derive(Debug, Clone)]
pub struct TouchTracker {
    need: u32,
    streak: u32,
    touching: bool,
}

impl TouchTracker {
    pub fn new(need: u32) -> Self {
        TouchTracker {
            need: need.max(1),
            streak: 0,
            touching: false,
        }
    }

    pub fn touching(&self) -> bool {
        self.touching
    }

    /// `over` — давление в этом отчёте выше порога. Возвращает текущее состояние касания.
    pub fn update(&mut self, over: bool) -> bool {
        if self.touching {
            if !over {
                self.touching = false;
                self.streak = 0;
            }
        } else if over {
            self.streak += 1;
            self.touching = self.streak >= self.need;
        } else {
            self.streak = 0;
        }
        self.touching
    }

    pub fn reset(&mut self) {
        self.streak = 0;
        self.touching = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_is_passthrough() {
        let mut f = PenFilter::new(0.0);
        assert_eq!(f.apply(10.0, 10.0), (10.0, 10.0));
        assert_eq!(f.apply(11.0, 9.0), (11.0, 9.0));
    }

    #[test]
    fn first_sample_after_reset_is_not_lagged() {
        let mut f = PenFilter::new(0.8);
        f.apply(0.0, 0.0);
        f.reset();
        assert_eq!(f.apply(1000.0, 1000.0), (1000.0, 1000.0));
    }

    #[test]
    fn jitter_is_reduced() {
        let mut f = PenFilter::new(0.6);
        f.apply(100.0, 100.0);
        let (x, _) = f.apply(101.0, 100.0); // шум в 1 единицу
        assert!(x < 100.8, "jitter should be damped, got {x}");
    }

    #[test]
    fn fast_motion_has_almost_no_lag() {
        let mut f = PenFilter::new(0.6);
        f.apply(0.0, 0.0);
        let (x, _) = f.apply(30.0, 0.0); // быстрый рывок
        assert!(x > 29.9, "fast move should pass through, got {x}");
    }

    #[test]
    fn single_spike_is_not_a_touch() {
        let mut t = TouchTracker::new(2);
        assert!(!t.update(true)); // одиночный всплеск
        assert!(!t.update(false));
        assert!(!t.update(true));
        assert!(t.update(true)); // два подряд — касание
        assert!(t.update(true));
        assert!(!t.update(false)); // отпускание — сразу
    }

    #[test]
    fn need_one_is_immediate() {
        let mut t = TouchTracker::new(1);
        assert!(t.update(true));
        assert!(!t.update(false));
    }

    #[test]
    fn reset_clears_state() {
        let mut t = TouchTracker::new(1);
        t.update(true);
        t.reset();
        assert!(!t.touching());
    }
}
