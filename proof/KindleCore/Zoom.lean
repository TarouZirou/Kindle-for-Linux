/-!
# Zoom

Formal model of `src/core/zoom.rs`. Zoom levels are integer percentages.
-/

namespace KindleCore

def zoomMin : Nat := 25
def zoomMax : Nat := 800

def presets : List Nat :=
  [25, 33, 50, 67, 75, 90, 100, 110, 125, 150, 175, 200, 250, 300, 400, 500, 600, 800]

def clampZoom (z : Nat) : Nat := max zoomMin (min z zoomMax)

/-- Next larger preset (or the maximum). -/
def zoomIn (z : Nat) : Nat := (presets.find? (fun x => decide (z < x))).getD zoomMax

/-- Next smaller preset (or the minimum). -/
def zoomOut (z : Nat) : Nat := (presets.reverse.find? (fun x => decide (x < z))).getD zoomMin

/-- Apply a zoom percentage to a length. -/
def scaleBy (len z : Nat) : Nat := len * z / 100

/-- Largest size with the aspect ratio of `iw × ih` that fits in `bw × bh`. -/
def fitSize (iw ih bw bh : Nat) : Nat × Nat :=
  if iw = 0 ∨ ih = 0 then (0, 0)
  else if iw * bh ≤ bw * ih then (iw * bh / ih, bh)
  else (bw, ih * bw / iw)

/-- Size that fills the width `bw` (used for vertical scroll and "fit width"). -/
def fitWidth (iw ih bw : Nat) : Nat × Nat :=
  if iw = 0 then (0, 0) else (bw, ih * bw / iw)

theorem presets_bounds : ∀ x ∈ presets, zoomMin ≤ x ∧ x ≤ zoomMax := by decide

theorem clampZoom_bounds (z : Nat) : zoomMin ≤ clampZoom z ∧ clampZoom z ≤ zoomMax := by
  simp only [clampZoom, zoomMin, zoomMax]; omega

theorem clampZoom_id (z : Nat) (h1 : zoomMin ≤ z) (h2 : z ≤ zoomMax) : clampZoom z = z := by
  simp only [clampZoom, zoomMin, zoomMax] at *; omega

theorem zoomIn_bounds (z : Nat) : zoomMin ≤ zoomIn z ∧ zoomIn z ≤ zoomMax := by
  unfold zoomIn
  cases h : presets.find? (fun x => decide (z < x)) with
  | none => simp [zoomMin, zoomMax]
  | some x => exact presets_bounds x (List.mem_of_find?_eq_some h)

theorem zoomOut_bounds (z : Nat) : zoomMin ≤ zoomOut z ∧ zoomOut z ≤ zoomMax := by
  unfold zoomOut
  cases h : presets.reverse.find? (fun x => decide (x < z)) with
  | none => simp [zoomMin, zoomMax]
  | some x =>
    exact presets_bounds x (List.mem_reverse.1 (List.mem_of_find?_eq_some h))

/-- Zooming in always makes things larger, unless already at the maximum. -/
theorem zoomIn_gt (z : Nat) (h : z < zoomMax) : z < zoomIn z := by
  unfold zoomIn
  cases hf : presets.find? (fun x => decide (z < x)) with
  | none =>
    have := List.find?_eq_none.1 hf 800 (by decide)
    simp [zoomMax] at h this; omega
  | some x =>
    have := List.find?_some hf
    simpa using this

/-- Zooming out always makes things smaller, unless already at the minimum. -/
theorem zoomOut_lt (z : Nat) (h : zoomMin < z) : zoomOut z < z := by
  unfold zoomOut
  cases hf : presets.reverse.find? (fun x => decide (x < z)) with
  | none =>
    have := List.find?_eq_none.1 hf 25 (by decide)
    simp [zoomMin] at h this; omega
  | some x =>
    have := List.find?_some hf
    simpa using this

/-- From any preset, zooming in and back out returns to the same preset. -/
theorem zoomOut_zoomIn : ∀ z ∈ presets, z ≠ zoomMax → zoomOut (zoomIn z) = z := by decide

theorem zoomIn_zoomOut : ∀ z ∈ presets, z ≠ zoomMin → zoomIn (zoomOut z) = z := by decide

/-- A fitted page never exceeds the box it is fitted into. -/
theorem fitSize_le (iw ih bw bh : Nat) :
    (fitSize iw ih bw bh).1 ≤ bw ∧ (fitSize iw ih bw bh).2 ≤ bh := by
  unfold fitSize
  split
  · simp
  · rename_i h0
    split
    · rename_i hle
      refine ⟨?_, Nat.le_refl _⟩
      have hih : 0 < ih := by omega
      exact (Nat.div_le_iff_le_mul_add_pred hih).2 (by
        rw [Nat.mul_comm bw ih] at hle; omega)
    · rename_i hgt
      refine ⟨Nat.le_refl _, ?_⟩
      have hiw : 0 < iw := by omega
      exact (Nat.div_le_iff_le_mul_add_pred hiw).2 (by
        rw [Nat.mul_comm ih bw] at *; omega)

/-- …and touches at least one side of it. -/
theorem fitSize_touches (iw ih bw bh : Nat) (h : 0 < iw ∧ 0 < ih) :
    (fitSize iw ih bw bh).1 = bw ∨ (fitSize iw ih bw bh).2 = bh := by
  unfold fitSize
  split
  · omega
  · split <;> simp

theorem scaleBy_100 (len : Nat) : scaleBy len 100 = len := by
  simp [scaleBy]

theorem scaleBy_mono (len z₁ z₂ : Nat) (h : z₁ ≤ z₂) : scaleBy len z₁ ≤ scaleBy len z₂ :=
  Nat.div_le_div_right (Nat.mul_le_mul_left len h)

end KindleCore
