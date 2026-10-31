/-
Copyright (c) 2026 Tiberiu Toca. All rights reserved.
Released under Apache 2.0 license as described in the file LICENSE-APACHE.
Authors: Tiberiu Toca
-/
import Lean

/-!
# Statement manifest

Prints one line per declaration of a library: its kind, its full name and its elaborated type,
followed by the body of a definition and the axioms a theorem depends on. The committed `statements.txt` is this output, and CI
regenerates it and fails on any difference, so a weakened statement, a dropped converse or an added
hypothesis is a visible diff even though the build, leanchecker and the axiom audit all pass on it.

The script also refuses, with exit code 1, any declared `axiom` in the library and any declaration
whose axioms go beyond `propext`, `Classical.choice` and `Quot.sound`. That is a second axiom check, owned
by this repository, next to the one `leanprover/lean-action` runs.

Run it from the package directory after `lake build`:

    lake env lean --run scripts/Statements.lean <RootModule> > statements.txt
-/

open Lean Meta

/-- The axioms a theorem may depend on. `sorryAx` and `Lean.ofReduceBool` are not among them. -/
def standardAxioms : List Name := [``propext, ``Classical.choice, ``Quot.sound]

/-- Declarations Lean generates for structures, inductives and equation lemmas. Their types follow
from the declarations that are listed, so listing them would only add noise to the diff. -/
def isGenerated (env : Environment) (name : Name) : Bool :=
  name.isInternalDetail || isAuxRecursor env name || isNoConfusion env name || isRecCore env name ||
    isMatcherCore env name ||
    match name with
    | .str _ last =>
      last ∈ ["injEq", "inj", "sizeOf_spec", "noConfusionType", "ctorIdx", "ctorElim",
        "ctorElimType", "congr_simp", "eq_def"] || last.startsWith "_" ||
        (last.startsWith "eq_" && (last.drop 3).all Char.isDigit)
    | _ => false

def kindOf : ConstantInfo → String
  | .thmInfo _ => "theorem"
  | .defnInfo _ => "def"
  | .axiomInfo _ => "axiom"
  | .opaqueInfo _ => "opaque"
  | .inductInfo _ => "inductive"
  | .ctorInfo _ => "constructor"
  | .recInfo _ => "recursor"
  | .quotInfo _ => "quot"

/-- The library's own declarations, in name order. Only the library's modules are read, so a
library that imports Mathlib does not walk Mathlib's constants. -/
def libraryDeclarations (env : Environment) (root : Name) : Array (Name × ConstantInfo) := Id.run do
  let mut found := #[]
  for i in [0:env.header.moduleNames.size] do
    let module := env.header.moduleNames[i]!
    if module == root || root.isPrefixOf module then
      for name in env.header.moduleData[i]!.constNames do
        if !isGenerated env name then
          if let some info := env.find? name then
            found := found.push (name, info)
  return found.qsort (fun a b => a.1.toString < b.1.toString)

def manifest (root : Name) : MetaM (Array String × Array String) := do
  let env ← getEnv
  let mut lines := #[]
  let mut refusals := #[]
  for (name, info) in libraryDeclarations env root do
    let type := (← ppExpr info.type).pretty 100000
    lines := lines.push s!"{kindOf info} {name} : {type}"
    -- A definition's body is part of what a statement about it means, so it is pinned too.
    -- Instances, their derived helpers and structure projections are generated, so are skipped.
    if let .defnInfo d := info then
      let derived := name.components.any fun part => part.toString.startsWith "inst"
      unless derived || (← isInstance name) || (env.isProjectionFn name) do
        lines := lines.push s!"  value: {(← ppExpr d.value).pretty 100000}"
    if let .axiomInfo _ := info then
      refusals := refusals.push s!"{name} is a declared axiom"
    let axioms := (← collectAxioms name).qsort (·.toString < ·.toString)
    if let .thmInfo _ := info then
      lines := lines.push s!"  axioms: {axioms.toList}"
    for ax in axioms do
      unless standardAxioms.contains ax do
        refusals := refusals.push s!"{name} depends on {ax}"
  return (lines, refusals)

unsafe def main (args : List String) : IO UInt32 := do
  let some rootArg := args.head? | IO.eprintln "usage: Statements <RootModule>"; return 2
  let root := rootArg.toName
  initSearchPath (← findSysroot)
  enableInitializersExecution
  let env ← importModules #[{ module := root }] {} (loadExts := true)
  let opts : Options := ({} : Options)
    |>.insert `format.width (.ofNat 100000) |>.insert `pp.fullNames (.ofBool true)
  let ctx : Core.Context := { fileName := "<statements>", fileMap := default, options := opts }
  let ((lines, refusals), _) ← (manifest root).run'.toIO ctx { env }
  for line in lines do IO.println line
  for refusal in refusals do IO.eprintln s!"refused: {refusal}"
  return if refusals.isEmpty then 0 else 1
