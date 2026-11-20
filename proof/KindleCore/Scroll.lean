/-!
# Scrolling, panning and position lookup

Formal model of `src/core/scroll.rs`.
-/

namespace KindleCore

/-- Clamp a scroll offset so the viewport never leaves the content. -/
def clampOffset (off content view : Nat) : Nat := min off (content - view)

/-- Offset that centres content smaller than the viewport. -/
def centerPad (content view : Nat) : Nat := (view - content) / 2

/-- Start offsets of stacked items with heights `hs`, separated by `gap`. -/
def starts : List Nat → Nat → Nat → List Nat
  | [], _, _ => []
  | h :: hs, gap, acc => acc :: starts hs gap (acc + h + gap)

/-- Total height of stacked items (including gaps between them). -/
def totalHeight (hs : List Nat) (gap : Nat) : Nat :=
  hs.foldl (· + ·) 0 + gap * (hs.length - 1)

/-- Index of the item containing position `x`, given ascending `starts`
(the number of starts `≤ x`, minus one). Used both for "which page is at the
top of the vertical scroll view" and for "which page holds this character
offset" after re-flowing a text book. -/
def locate (ss : List Nat) (x : Nat) : Nat :=
  (ss.filter (fun s => decide (s ≤ x))).length - 1

theorem clampOffset_le (off content view : Nat) : clampOffset off content view ≤ off :=
  Nat.min_le_left _ _

/-- The viewport stays inside the content. -/
theorem clampOffset_fits (off content view : Nat) (h : view ≤ content) :
    clampOffset off content view + view ≤ content := by
  unfold clampOffset; omega

/-- Content smaller than the viewport can't be scrolled at all. -/
theorem clampOffset_small (off content view : Nat) (h : content ≤ view) :
    clampOffset off content view = 0 := by
  unfold clampOffset; omega

theorem clampOffset_idem (off content view : Nat) :
    clampOffset (clampOffset off content view) content view = clampOffset off content view := by
  unfold clampOffset; omega

theorem centerPad_fits (content view : Nat) (h : content ≤ view) :
    centerPad content view + content ≤ view := by
  unfold centerPad; omega

theorem starts_length (hs : List Nat) (gap acc : Nat) : (starts hs gap acc).length = hs.length := by
  induction hs generalizing acc with
  | nil => rfl
  | cons h t ih => simp [starts, ih]

theorem starts_head (h : Nat) (hs : List Nat) (gap acc : Nat) :
    (starts (h :: hs) gap acc).head? = some acc := rfl

/-- `locate` always returns a valid index (for a non-empty list). -/
theorem locate_lt (ss : List Nat) (x : Nat) (h : ss ≠ []) : locate ss x < ss.length := by
  unfold locate
  have h1 : (ss.filter (fun s => decide (s ≤ x))).length ≤ ss.length := List.length_filter_le _ _
  have h2 : 0 < ss.length := List.length_pos_iff.2 h
  omega

theorem locate_mono (ss : List Nat) (x y : Nat) (hxy : x ≤ y) : locate ss x ≤ locate ss y := by
  unfold locate
  have : (ss.filter (fun s => decide (s ≤ x))).length ≤ (ss.filter (fun s => decide (s ≤ y))).length := by
    induction ss with
    | nil => simp
    | cons a t ih =>
      simp only [List.filter_cons]
      by_cases ha : a ≤ x
      · have : a ≤ y := by omega
        simp [ha, this]; omega
      · by_cases hb : a ≤ y
        · simp [ha, hb]; omega
        · simp [ha, hb]; omega
  omega

end KindleCore
