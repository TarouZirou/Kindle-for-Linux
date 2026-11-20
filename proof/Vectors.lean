import KindleCore

/-!
Prints input/output vectors computed by the *verified* Lean definitions.
`tests/lean_vectors.rs` replays every line against the Rust implementation
in `src/core`, so the Rust code must agree with the Lean model on all of them.

    lake env lean --run Vectors.lean > ../tests/vectors.txt
-/

open KindleCore

def modes : List Mode := [.single, .spreadLTR, .spreadRTL, .vertical]

def modeId : Mode → Nat
  | .single => 0 | .spreadLTR => 1 | .spreadRTL => 2 | .vertical => 3

def showList (l : List Nat) : String :=
  if l.isEmpty then "-" else ",".intercalate (l.map toString)

def line (xs : List String) : String := " ".intercalate xs

def main : IO Unit := do
  -- Layout & navigation
  for m in modes do
    for c in [false, true] do
      for n in List.range 11 do
        for p in List.range 12 do
          IO.println <| line ["L", toString (modeId m), toString (if c then 1 else 0), toString n, toString p,
            toString (spreadOf c p), toString (spreadCount c n), toString (normalize m c n p),
            toString (nextPage m c n p), toString (prevPage m c n p), toString (lastVisible m c n p),
            showList (visible m c n p), showList (displayOrder m c n p)]
  for n in List.range 14 do
    for k in (List.range 28).map (· * 41) do
      IO.println <| line ["P", toString n, toString k, toString (pageForPermille n k)]
  -- Zoom
  for z in List.range 901 do
    IO.println <| line ["Z", toString z, toString (clampZoom z), toString (zoomIn z), toString (zoomOut z),
      toString (scaleBy 1234 z)]
  let dims := [0, 1, 7, 100, 333, 640, 1080, 1920]
  for iw in dims do
    for ih in dims do
      for bw in dims do
        for bh in dims do
          let (w, h) := fitSize iw ih bw bh
          let (fw, fh) := fitWidth iw ih bw
          IO.println <| line ["F", toString iw, toString ih, toString bw, toString bh,
            toString w, toString h, toString fw, toString fh]
  -- Scroll
  for off in [0, 1, 50, 99, 100, 101, 500, 5000] do
    for content in [0, 10, 100, 480, 1000, 4000] do
      for view in [0, 10, 100, 480, 1000] do
        IO.println <| line ["S", toString off, toString content, toString view,
          toString (clampOffset off content view), toString (centerPad content view)]
  let heights : List (List Nat) := [[], [10], [100, 200], [5, 5, 5, 5], [300, 0, 120, 999, 1]]
  for hs in heights do
    for gap in [0, 8] do
      let ss := starts hs gap 0
      IO.println <| line ["ST", showList hs, toString gap, showList ss, toString (totalHeight hs gap)]
      for x in (List.range 30).map (· * 47) do
        IO.println <| line ["LOC", showList ss, toString x, toString (locate ss x)]
  -- Progress
  for n in List.range 25 do
    for r in List.range 27 do
      for est in [1000, 45000] do
        IO.println <| line ["PCT", toString r, toString n, toString est, toString (percent r n),
          toString (remainingPages r n), toString (remainingMs r n est)]
  for est in [1000, 5000, 45000, 600000] do
    for e in [0, 999, 1000, 4000, 30000, 61000, 600000, 600001, 1300000] do
      for p in [0, 1, 2, 3] do
        IO.println <| line ["SPD", toString est, toString e, toString p, toString (updateSpeed est e p)]
  for ms in [0, 1, 59999, 60000, 60001, 3599999, 3600000, 3600001, 5400000, 86400000, 123456789] do
    let (h, m) := hoursMinutes ms
    IO.println <| line ["HM", toString ms, toString h, toString m, toString (clampSpeed ms)]
