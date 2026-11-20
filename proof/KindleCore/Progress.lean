/-!
# Reading progress and time-to-finish estimate

Formal model of `src/core/progress.rs`.

The reading speed is an exponential moving average of milliseconds per page,
updated on each page turn. Samples that are implausibly fast (skimming /
flipping, `< 1 s` per page) or slow (the reader walked away, `> 10 min`) are
ignored.
-/

namespace KindleCore

def msMin : Nat := 1000
def msMax : Nat := 600000

/-- Percentage read when `reached` (0-based) is the furthest visible page. -/
def percent (reached n : Nat) : Nat :=
  if n = 0 then 0 else min (reached + 1) n * 100 / n

/-- Pages left after the furthest visible page. -/
def remainingPages (reached n : Nat) : Nat := n - (reached + 1)

def clampSpeed (ms : Nat) : Nat := max msMin (min ms msMax)

/-- Per-page time of one page turn, if plausible. -/
def acceptSample (elapsedMs pages : Nat) : Option Nat :=
  if pages = 0 then none
  else
    let s := elapsedMs / pages
    if s < msMin ∨ msMax < s then none else some s

/-- Update the reading speed estimate (EMA, weight 3/10 for the new sample). -/
def updateSpeed (est elapsedMs pages : Nat) : Nat :=
  match acceptSample elapsedMs pages with
  | none => est
  | some s => (7 * est + 3 * s) / 10

def remainingMs (reached n est : Nat) : Nat := remainingPages reached n * est

/-- Whole minutes, rounded up, so that "0 min" is only shown when done. -/
def ceilMinutes (ms : Nat) : Nat := (ms + 59999) / 60000

/-- `(hours, minutes)` for display: 「あと h 時間 m 分」. -/
def hoursMinutes (ms : Nat) : Nat × Nat := (ceilMinutes ms / 60, ceilMinutes ms % 60)

/-! ## Percent -/

theorem percent_le_100 (reached n : Nat) : percent reached n ≤ 100 := by
  unfold percent
  split
  · omega
  · rename_i hn
    have hpos : 0 < n := by omega
    exact (Nat.div_le_iff_le_mul_add_pred hpos).2 (by
      have : min (reached + 1) n * 100 ≤ n * 100 := Nat.mul_le_mul_right _ (Nat.min_le_right _ _)
      omega)

/-- The last page shows 100 %. -/
theorem percent_end (n : Nat) (hn : 0 < n) : percent (n - 1) n = 100 := by
  unfold percent
  have : ¬ n = 0 := by omega
  simp only [this, if_false]
  have : min (n - 1 + 1) n = n := by omega
  rw [this]; exact Nat.mul_div_cancel_left 100 hn

theorem percent_mono (r₁ r₂ n : Nat) (h : r₁ ≤ r₂) : percent r₁ n ≤ percent r₂ n := by
  unfold percent
  split
  · omega
  · apply Nat.div_le_div_right
    apply Nat.mul_le_mul_right
    omega

/-- Before the last page, progress is never reported as 100 %. -/
theorem percent_lt_100 (reached n : Nat) (h : reached + 1 < n) : percent reached n < 100 := by
  unfold percent
  have hn : ¬ n = 0 := by omega
  simp only [hn, if_false]
  have : min (reached + 1) n = reached + 1 := by omega
  rw [this]
  have hpos : 0 < n := by omega
  exact (Nat.div_lt_iff_lt_mul hpos).2 (by rw [Nat.mul_comm 100 n]; exact Nat.mul_lt_mul_of_pos_right h (by decide))

/-! ## Reading speed -/

theorem acceptSample_bounds (e p s : Nat) (h : acceptSample e p = some s) :
    msMin ≤ s ∧ s ≤ msMax := by
  unfold acceptSample at h
  split at h
  · simp at h
  · simp only at h
    split at h
    · simp at h
    · rename_i hb
      simp at h
      omega

/-- The speed estimate never leaves `[msMin, msMax]`. -/
theorem updateSpeed_bounds (est e p : Nat) (h : msMin ≤ est ∧ est ≤ msMax) :
    msMin ≤ updateSpeed est e p ∧ updateSpeed est e p ≤ msMax := by
  unfold updateSpeed
  cases hs : acceptSample e p with
  | none => exact h
  | some s =>
    have := acceptSample_bounds e p s hs
    simp only [msMin, msMax] at *
    omega

theorem clampSpeed_bounds (ms : Nat) : msMin ≤ clampSpeed ms ∧ clampSpeed ms ≤ msMax := by
  simp only [clampSpeed, msMin, msMax]; omega

/-! ## Time remaining -/

theorem remainingMs_end (n est : Nat) : remainingMs (n - 1) n est = 0 := by
  unfold remainingMs remainingPages
  have : n - (n - 1 + 1) = 0 := by omega
  rw [this, Nat.zero_mul]

theorem remainingMs_anti (r₁ r₂ n est : Nat) (h : r₁ ≤ r₂) :
    remainingMs r₂ n est ≤ remainingMs r₁ n est := by
  unfold remainingMs remainingPages
  exact Nat.mul_le_mul_right _ (by omega)

/-- No overflow in the Rust `u64` implementation: with at most a million
pages the result stays far below `2^64`. -/
theorem remainingMs_bounded (r n est : Nat) (hn : n ≤ 1000000) (he : est ≤ msMax) :
    remainingMs r n est ≤ 1000000 * 600000 := by
  unfold remainingMs remainingPages
  exact Nat.mul_le_mul (by omega) he

theorem hoursMinutes_min_lt (ms : Nat) : (hoursMinutes ms).2 < 60 := by
  simp [hoursMinutes]; omega

theorem hoursMinutes_total (ms : Nat) :
    (hoursMinutes ms).1 * 60 + (hoursMinutes ms).2 = ceilMinutes ms := by
  simp [hoursMinutes]; omega

/-- If there is anything left to read, at least "1 min" is displayed. -/
theorem ceilMinutes_pos (ms : Nat) (h : 0 < ms) : 0 < ceilMinutes ms := by
  unfold ceilMinutes; omega

theorem ceilMinutes_zero : ceilMinutes 0 = 0 := by decide

/-- The displayed time is never less than the real estimate. -/
theorem ceilMinutes_ge (ms : Nat) : ms ≤ ceilMinutes ms * 60000 := by
  unfold ceilMinutes; omega

end KindleCore
