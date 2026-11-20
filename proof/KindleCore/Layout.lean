/-!
# Page layout and navigation

Formal model of `src/core/layout.rs`.

* `single`    : one page at a time
* `spreadLTR` : two pages side by side, read left → right (Western books)
* `spreadRTL` : two pages side by side, read right → left (manga / 縦書き books)
* `vertical`  : continuous vertical scroll (navigation behaves like `single`)

With `cover = true` the first page (the cover) is shown on its own and the
following pages are paired as `[1,2] [3,4] …`; otherwise `[0,1] [2,3] …`.
-/

namespace KindleCore

inductive Mode where
  | single
  | spreadLTR
  | spreadRTL
  | vertical
  deriving Repr, DecidableEq

def Mode.isSpread : Mode → Bool
  | .spreadLTR => true
  | .spreadRTL => true
  | _ => false

/-- `1` if the cover stands alone, else `0`. -/
def shift (cover : Bool) : Nat := if cover then 1 else 0

/-- Index of the spread containing page `p`. -/
def spreadOf (cover : Bool) (p : Nat) : Nat := (p + shift cover) / 2

/-- First page of spread `s`. -/
def spreadStart (cover : Bool) (s : Nat) : Nat := 2 * s - shift cover

/-- One past the last page of spread `s` (ignoring the end of the book). -/
def spreadEnd (cover : Bool) (s : Nat) : Nat := 2 * s + 2 - shift cover

/-- Number of spreads in a book of `n` pages. -/
def spreadCount (cover : Bool) (n : Nat) : Nat :=
  if n = 0 then 0 else (n + shift cover + 1) / 2

/-- Pages of spread `s` in reading order. -/
def spreadPages (cover : Bool) (n s : Nat) : List Nat :=
  List.range' (spreadStart cover s) (min (spreadEnd cover s) n - spreadStart cover s)

/-- First page of the unit (page or spread) that contains `p`. -/
def anchor (m : Mode) (cover : Bool) (p : Nat) : Nat :=
  if m.isSpread then spreadStart cover (spreadOf cover p) else p

/-- Bring any stored page number into range and onto a unit boundary. -/
def normalize (m : Mode) (cover : Bool) (n p : Nat) : Nat :=
  anchor m cover (min p (n - 1))

def nextPage (m : Mode) (cover : Bool) (n p : Nat) : Nat :=
  if m.isSpread then
    if spreadOf cover p + 1 < spreadCount cover n then
      spreadStart cover (spreadOf cover p + 1)
    else p
  else if p + 1 < n then p + 1 else p

def prevPage (m : Mode) (cover : Bool) (_n p : Nat) : Nat :=
  if m.isSpread then spreadStart cover (spreadOf cover p - 1)
  else p - 1

/-- Pages visible on screen, in reading order. -/
def visible (m : Mode) (cover : Bool) (n p : Nat) : List Nat :=
  if m.isSpread then spreadPages cover n (spreadOf cover p)
  else if p < n then [p] else []

/-- Pages visible on screen, in left → right screen order. -/
def displayOrder (m : Mode) (cover : Bool) (n p : Nat) : List Nat :=
  if m = .spreadRTL then (visible m cover n p).reverse else visible m cover n p

/-- The furthest page the reader has reached when `p` is the anchor. -/
def lastVisible (m : Mode) (cover : Bool) (n p : Nat) : Nat :=
  if m.isSpread then min (spreadEnd cover (spreadOf cover p)) n - 1 else p

/-- Map a slider position `k ∈ [0,1000]` to a page. -/
def pageForPermille (n k : Nat) : Nat := min (min k 1000 * n / 1000) (n - 1)

/-! ## Spreads partition the book -/

theorem shift_le (cover : Bool) : shift cover ≤ 1 := by
  cases cover <;> simp [shift]

/-- Page `p` is in spread `s` exactly when `spreadOf p = s`:
every page belongs to exactly one spread. -/
theorem mem_spreadPages (cover : Bool) (n s p : Nat) :
    p ∈ spreadPages cover n s ↔ p < n ∧ spreadOf cover p = s := by
  have h := shift_le cover
  simp only [spreadPages, List.mem_range'_1, spreadOf, spreadStart, spreadEnd]
  omega

theorem spreadPages_lt (cover : Bool) (n s p : Nat) (h : p ∈ spreadPages cover n s) : p < n :=
  ((mem_spreadPages cover n s p).1 h).1

theorem spreadPages_length_le (cover : Bool) (n s : Nat) :
    (spreadPages cover n s).length ≤ 2 := by
  have h := shift_le cover
  simp only [spreadPages, List.length_range', spreadStart, spreadEnd]
  omega

theorem spreadOf_lt_count (cover : Bool) (n p : Nat) (hp : p < n) :
    spreadOf cover p < spreadCount cover n := by
  have h := shift_le cover
  simp only [spreadOf, spreadCount]
  split <;> omega

/-- Every spread index below `spreadCount` has at least one page. -/
theorem spreadPages_ne_nil (cover : Bool) (n s : Nat) (hs : s < spreadCount cover n) :
    spreadPages cover n s ≠ [] := by
  have h := shift_le cover
  intro hnil
  have : (spreadPages cover n s).length = 0 := by simp [hnil]
  simp only [spreadPages, List.length_range', spreadStart, spreadEnd] at this
  simp only [spreadCount] at hs
  split at hs <;> omega

theorem spreadStart_le (cover : Bool) (p : Nat) :
    spreadStart cover (spreadOf cover p) ≤ p := by
  have h := shift_le cover
  simp only [spreadStart, spreadOf]
  omega

/-! ## Navigation stays inside the book -/

theorem anchor_le (m : Mode) (cover : Bool) (p : Nat) : anchor m cover p ≤ p := by
  unfold anchor; split
  · exact spreadStart_le cover p
  · exact Nat.le_refl p

theorem normalize_lt (m : Mode) (cover : Bool) (n p : Nat) (hn : 0 < n) :
    normalize m cover n p < n := by
  have := anchor_le m cover (min p (n - 1))
  unfold normalize; omega

theorem nextPage_lt (m : Mode) (cover : Bool) (n p : Nat) (hp : p < n) :
    nextPage m cover n p < n := by
  have h := shift_le cover
  unfold nextPage
  split
  · split
    · rename_i h1
      simp only [spreadCount, spreadStart, spreadOf] at h1 ⊢
      split at h1 <;> omega
    · exact hp
  · split <;> omega

theorem nextPage_ge (m : Mode) (cover : Bool) (n p : Nat) :
    p ≤ nextPage m cover n p := by
  have h := shift_le cover
  unfold nextPage
  split
  · split
    · simp only [spreadStart, spreadOf]; omega
    · exact Nat.le_refl p
  · split <;> omega

theorem prevPage_le (m : Mode) (cover : Bool) (n p : Nat) :
    prevPage m cover n p ≤ p := by
  have h := shift_le cover
  unfold prevPage
  split
  · simp only [spreadStart, spreadOf]; omega
  · omega

theorem prevPage_lt (m : Mode) (cover : Bool) (n p : Nat) (hp : p < n) :
    prevPage m cover n p < n :=
  Nat.lt_of_le_of_lt (prevPage_le m cover n p) hp

/-- Going forward and back again returns to the same page (single mode). -/
theorem prev_next_single (n p : Nat) (h : p + 1 < n) :
    prevPage .single c n (nextPage .single c n p) = p := by
  simp [nextPage, prevPage, Mode.isSpread, h]

/-- Going forward and back again returns to the same spread anchor. -/
theorem prev_next_spread (m : Mode) (hm : m.isSpread = true) (cover : Bool) (n p : Nat)
    (hlast : spreadOf cover p + 1 < spreadCount cover n) :
    prevPage m cover n (nextPage m cover n p) = anchor m cover p := by
  have h := shift_le cover
  simp only [nextPage, prevPage, anchor, hm, hlast, if_true]
  simp only [spreadStart, spreadOf]
  omega

/-! ## What is on screen -/

theorem visible_lt (m : Mode) (cover : Bool) (n p q : Nat) (hq : q ∈ visible m cover n p) :
    q < n := by
  unfold visible at hq
  split at hq
  · exact spreadPages_lt cover n _ q hq
  · split at hq
    · simp at hq; omega
    · simp at hq

theorem visible_length_le (m : Mode) (cover : Bool) (n p : Nat) :
    (visible m cover n p).length ≤ 2 := by
  unfold visible; split
  · exact spreadPages_length_le cover n _
  · split <;> simp

/-- The current page is always on screen. -/
theorem mem_visible (m : Mode) (cover : Bool) (n p : Nat) (hp : p < n) :
    p ∈ visible m cover n p := by
  unfold visible; split
  · exact (mem_spreadPages cover n _ p).2 ⟨hp, rfl⟩
  · simp

/-- Right-to-left spreads are the mirror image of left-to-right spreads. -/
theorem rtl_mirror (cover : Bool) (n p : Nat) :
    displayOrder .spreadRTL cover n p = (displayOrder .spreadLTR cover n p).reverse := by
  simp [displayOrder, visible, Mode.isSpread]

theorem lastVisible_lt (m : Mode) (cover : Bool) (n p : Nat) (hp : p < n) :
    lastVisible m cover n p < n := by
  unfold lastVisible; split <;> omega

theorem le_lastVisible (m : Mode) (cover : Bool) (n p : Nat) (hp : p < n) :
    p ≤ lastVisible m cover n p := by
  have h := shift_le cover
  unfold lastVisible; split
  · simp only [spreadEnd, spreadOf]; omega
  · exact Nat.le_refl p

theorem pageForPermille_lt (n k : Nat) (hn : 0 < n) : pageForPermille n k < n := by
  unfold pageForPermille; omega

end KindleCore
