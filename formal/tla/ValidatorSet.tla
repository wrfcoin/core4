----------------------------- MODULE ValidatorSet -----------------------------
(***************************************************************************)
(* TLA+ model of validator-set changes across epoch boundaries, and the     *)
(* finality decisions taken while a set is rotating.                        *)
(*                                                                          *)
(* Story #2099, epic #2093.                                                 *)
(*                                                                          *)
(* ======================================================================== *)
(* WHAT THIS MODELS, AND WHAT IT DOES NOT                                   *)
(* ======================================================================== *)
(*                                                                          *)
(* This is a model of the DESIGNED protocol, not of the running testnet.    *)
(* Two facts make that distinction load-bearing, and both were verified in  *)
(* the Rust rather than assumed:                                            *)
(*                                                                          *)
(*   1. The finality gadget is NOT ATTACHED on the production path.         *)
(*      `native-chain/src/bin/launch_api/main.rs:961` sets                  *)
(*      `bft_finality_for_p2p = None`, and the comment above it says the    *)
(*      intended `ProofOfStake::attach_bft_finality` wiring is future work. *)
(*      The testnet runs PoP. So NOTHING in this spec describes a decision  *)
(*      the deployed node makes today.                                      *)
(*                                                                          *)
(*   2. The gadget has NO SET-ROTATION API. `BftFinalityGadget::new`        *)
(*      (`bft_finality.rs:344-353`) computes `total_weight` once by summing *)
(*      the constructor's validators, and no method rotates that set.       *)
(*      Epoch-boundary rotation is therefore a property of the DESIGN; the  *)
(*      built gadget cannot express it.                                     *)
(*                                                                          *)
(* An earlier revision of this spec asserted `PowerConservation` over an    *)
(* incrementally-maintained running total, described as mirroring           *)
(* production. It did not: production sums once and never updates. That     *)
(* invariant proved an implementation path that does not exist, so the      *)
(* running total is gone and totals are recomputed from the active set.     *)
(*                                                                          *)
(* WEIGHTING. This model is STAKE-WEIGHTED, matching Rust: `power[v]` in    *)
(* `1..MaxPower` mirrors `FinalityValidator::stake_weight`, and             *)
(* `IsSupermajority` sums weights exactly as `bft_finality.rs:511` does.    *)
(* This is a real difference from the count-weighted finality spec in       *)
(* #2255 — see ValidatorSet.README.md.                                      *)
(*                                                                          *)
(* DERIVED FROM, at commit b0bd2060c:                                       *)
(*   consensus-engines/src/selection/validator_set_source.rs                *)
(*       - validators_from_state (line 41): membership is                   *)
(*         `stake_locked >= min_stake`, weight is stake saturated to u64    *)
(*   consensus-engines/src/selection/bft_finality.rs                        *)
(*       - is_supermajority (line 511):  accumulated * 3 > total * 2        *)
(*       - required_weight  (line 516):  (total * 2 \div 3) + 1             *)
(*       - new              (line 344):  total_weight summed ONCE           *)
(*       - FinalityVote     (line 80)  and content_hash (line 94)           *)
(*                                                                          *)
(* WHY SMALL SETS ARE THE POINT                                             *)
(*                                                                          *)
(* Quorum-rounding mistakes hide at |V| = 1, 2 and 3, where integer         *)
(* division behaves least like the "2/3 of the validators" intuition. A     *)
(* 12-node testnet never exercises them. The configurations run every size  *)
(* from 1 to 4 explicitly.                                                  *)
(*                                                                          *)
(* THE TWO MODELLED DEFECTS                                                 *)
(*                                                                          *)
(* `EnforceVoteEpoch` — `FinalityVote` carries `block_hash`, `block_height` *)
(* and `validator_id`, and `content_hash` binds exactly those three. There  *)
(* is no epoch and no validator-set identity, so a vote cast under one set  *)
(* is byte-identical to the same vote replayed under the next.              *)
(*   TRUE  — votes are epoch-scoped and stale votes are refused (design).   *)
(*   FALSE — votes carry no epoch, as today (#2256).                        *)
(*                                                                          *)
(* `GadgetTracksSet` — whether the finality gadget's view of the validator  *)
(* set follows an epoch rotation.                                           *)
(*   TRUE  — the gadget is refreshed at each boundary (design).             *)
(*   FALSE — the gadget keeps its construction-time set and total, as       *)
(*           `BftFinalityGadget::new` does today. Run                       *)
(*           ValidatorSet_StaleGadget.cfg to see TLC finalize a block on a  *)
(*           minority of the real stake.                                    *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS
    Validators,         \* universe of candidate validator identities
    MaxEpoch,           \* highest epoch modelled
    MaxPower,           \* highest voting power a single validator may hold
    EnforceVoteEpoch,   \* TRUE = votes are epoch-scoped; FALSE = today's Rust
    GadgetTracksSet     \* TRUE = gadget follows rotation; FALSE = today's Rust

ASSUME MaxEpoch \in Nat /\ MaxEpoch >= 1
ASSUME MaxPower \in Nat /\ MaxPower >= 1
ASSUME EnforceVoteEpoch \in BOOLEAN
ASSUME GadgetTracksSet \in BOOLEAN

VARIABLES
    epoch,          \* current epoch
    activeSet,      \* SUBSET Validators — the set authorising votes NOW
    pendingSet,     \* SUBSET Validators — takes effect at the next boundary
    pendingPower,   \* staged weights; committed to `power` only at activation
    power,          \* [Validators -> 0..MaxPower]; 0 means not a member
    gadgetSet,      \* the gadget's frozen member set (bft_finality.rs `validators`)
    gadgetPower,    \* the gadget's FROZEN per-member weights (stake_weight)
    gadgetTotal,    \* the gadget's FROZEN total_weight, summed once in `new()`
    votes,          \* set of [voter, ep] votes ever cast
    counted,        \* SUBSET votes — accumulated toward the current quorum
    finalRecs       \* finalisation records: [ep, backing, realTotal]

vars == <<epoch, activeSet, pendingSet, pendingPower, power, gadgetSet, gadgetPower,
          gadgetTotal, votes, counted, finalRecs>>

Votes == [voter : Validators, ep : 1..MaxEpoch]

RECURSIVE SumPower(_)
SumPower(S) ==
    IF S = {} THEN 0
    ELSE LET v == CHOOSE x \in S : TRUE
         IN power[v] + SumPower(S \ {v})

(***************************************************************************)
(* Threshold arithmetic, copied from bft_finality.rs. Integer division      *)
(* (\div) is TLA+'s floor division, matching Rust's `/` on unsigned ints —  *)
(* the rounding behaviour is the thing under test, so it must not be        *)
(* "cleaned up" into rational arithmetic.                                   *)
(***************************************************************************)
IsSupermajority(accumulated, total) == accumulated * 3 > total * 2
RequiredWeight(total) == ((total * 2) \div 3) + 1

\* Weight actually backing the current quorum: only CURRENT members count.
CountedWeight ==
    LET voters == { m.voter : m \in counted }
    IN SumPower(voters \cap activeSet)

RECURSIVE SumPendingPower(_)
SumPendingPower(S) ==
    IF S = {} THEN 0
    ELSE LET v == CHOOSE x \in S : TRUE
         IN pendingPower[v] + SumPendingPower(S \ {v})

RECURSIVE SumFrozen(_)
SumFrozen(S) ==
    IF S = {} THEN 0
    ELSE LET v == CHOOSE x \in S : TRUE
         IN gadgetPower[v] + SumFrozen(S \ {v})

\* What the GADGET accumulates: its own frozen weights over its own frozen
\* membership -- exactly `add_vote`'s `self.validators.get(id).stake_weight`.
GadgetBacking ==
    LET voters == { m.voter : m \in counted }
    IN SumFrozen(voters \cap gadgetSet)

\* The total that actually exists on chain right now.
RealTotal == SumPower(activeSet)

Init ==
    /\ epoch = 1
    /\ activeSet = {}
    /\ pendingSet = {}
    /\ pendingPower = [v \in Validators |-> 0]
    /\ power = [v \in Validators |-> 0]
    /\ gadgetSet = {}
    /\ gadgetPower = [v \in Validators |-> 0]
    /\ gadgetTotal = 0
    /\ votes = {}
    /\ counted = {}
    /\ finalRecs = {}

-----------------------------------------------------------------------------
(***************************************************************************)
(* ACTIONS                                                                 *)
(***************************************************************************)

(***************************************************************************)
(* Set changes stage into `pendingSet` and take effect only at the next     *)
(* epoch boundary. This is the handoff the story asks about: a vote         *)
(* straddling the boundary is authorised by whichever set is ACTIVE in the  *)
(* epoch the vote is counted in, never by the pending one.                  *)
(*                                                                          *)
(* This mirrors PoS's boundary reconciliation (`pos.rs:445-475`) rather     *)
(* than the gadget, which has no rotation API at all.                       *)
(***************************************************************************)
StageAdd ==
    /\ epoch <= MaxEpoch
    /\ \E v \in Validators \ pendingSet, p \in 1..MaxPower :
        /\ pendingSet' = pendingSet \cup {v}
        /\ pendingPower' = [pendingPower EXCEPT ![v] = p]
    /\ UNCHANGED <<epoch, activeSet, power, gadgetSet, gadgetPower,
                   gadgetTotal, votes, counted, finalRecs>>

StageRemove ==
    /\ epoch <= MaxEpoch
    /\ \E v \in pendingSet :
        /\ pendingSet' = pendingSet \ {v}
        /\ pendingPower' = [pendingPower EXCEPT ![v] = 0]
    /\ UNCHANGED <<epoch, activeSet, power, gadgetSet, gadgetPower,
                   gadgetTotal, votes, counted, finalRecs>>

\* A member of the ACTIVE set casts a vote, tagged with the epoch it was
\* cast in. The tag exists in the model even when EnforceVoteEpoch is FALSE;
\* what changes is whether anyone checks it.
CastVote ==
    /\ epoch <= MaxEpoch
    /\ \E v \in activeSet :
        /\ ~ \E m \in votes : m.voter = v /\ m.ep = epoch
        /\ votes' = votes \cup {[voter |-> v, ep |-> epoch]}
    /\ UNCHANGED <<epoch, activeSet, pendingSet, pendingPower, power, gadgetSet, gadgetPower,
                   gadgetTotal, counted, finalRecs>>

(***************************************************************************)
(* Delivery. A vote may arrive late — cast under epoch E, delivered under   *)
(* E+1. With EnforceVoteEpoch it is refused. Without it, as today, nothing  *)
(* distinguishes it from a fresh vote.                                      *)
(***************************************************************************)
DeliverVote ==
    /\ epoch <= MaxEpoch
    /\ \E m \in votes \ counted :
        /\ EnforceVoteEpoch => m.ep = epoch
        /\ counted' = counted \cup {m}
    /\ UNCHANGED <<epoch, activeSet, pendingSet, pendingPower, power, gadgetSet, gadgetPower, gadgetTotal,
                   votes, finalRecs>>

(***************************************************************************)
(* FINALIZE — the action the previous revision was missing entirely.       *)
(*                                                                         *)
(* The gadget decides using ITS OWN view of the total (`GadgetTotal`),     *)
(* because `BftFinalityGadget` compares against the `total_weight` it      *)
(* computed at construction. The record also stores the REAL total, so the *)
(* safety invariant can ask whether the decision was sound against the set *)
(* that actually existed.                                                  *)
(*                                                                         *)
(* This is what makes a set change concurrent with finalisation reachable: *)
(* StageAdd/StageRemove, AdvanceEpoch and Finalize are all enabled         *)
(* independently, so TLC explores every interleaving of them.              *)
(***************************************************************************)
Finalize ==
    /\ epoch <= MaxEpoch
    \* The gadget must exist. Without this, `gadgetSet = {}` gives a total of
    \* 0 and ANY positive weight is trivially a supermajority — TLC found
    \* exactly that degenerate trace before this conjunct was added, which
    \* would have "proved" the negative control for the wrong reason.
    /\ gadgetSet /= {}
    /\ IsSupermajority(GadgetBacking, gadgetTotal)
    /\ ~ \E r \in finalRecs : r.ep = epoch
    /\ finalRecs' = finalRecs \cup
         {[ep |-> epoch, backing |-> CountedWeight, realTotal |-> RealTotal]}
    /\ UNCHANGED <<epoch, activeSet, pendingSet, pendingPower, power, gadgetSet, gadgetPower, gadgetTotal,
                   votes, counted>>

(***************************************************************************)
(* Advance the epoch. The pending set becomes active, non-members are      *)
(* zeroed, and the quorum accumulator is cleared. Votes already CAST       *)
(* remain in the network and can still be delivered late.                  *)
(*                                                                         *)
(* The gadget's view follows only if GadgetTracksSet — otherwise it keeps  *)
(* the set it was constructed with, as production does.                    *)
(***************************************************************************)
AdvanceEpoch ==
    /\ epoch < MaxEpoch
    /\ epoch' = epoch + 1
    /\ activeSet' = pendingSet
    /\ power' = [v \in Validators |->
                    IF v \in pendingSet THEN pendingPower[v] ELSE 0]
    /\ gadgetSet'   = IF GadgetTracksSet THEN pendingSet ELSE gadgetSet
    /\ gadgetPower' = IF GadgetTracksSet
                       THEN [v \in Validators |->
                                IF v \in pendingSet THEN pendingPower[v] ELSE 0]
                       ELSE gadgetPower
    /\ gadgetTotal' = IF GadgetTracksSet
                       THEN SumPendingPower(pendingSet)
                       ELSE gadgetTotal
    /\ counted' = {}
    /\ UNCHANGED <<pendingSet, pendingPower, votes, finalRecs>>

(***************************************************************************)
(* Gadget construction. The gadget is built once from whatever set is      *)
(* active at that moment — `BftFinalityGadget::new`. Modelled as an action *)
(* so the "constructed, then the set moved on" state is reachable.         *)
(***************************************************************************)
ConstructGadget ==
    /\ epoch <= MaxEpoch
    /\ gadgetSet = {}
    /\ activeSet /= {}
    /\ gadgetSet'   = activeSet
    /\ gadgetPower' = power
    /\ gadgetTotal' = SumPower(activeSet)
    /\ UNCHANGED <<epoch, activeSet, pendingSet, pendingPower, power, votes, counted,
                   finalRecs>>

Done ==
    /\ epoch >= MaxEpoch
    /\ UNCHANGED vars

Next ==
    \/ StageAdd
    \/ StageRemove
    \/ CastVote
    \/ DeliverVote
    \/ Finalize
    \/ AdvanceEpoch
    \/ ConstructGadget
    \/ Done

Spec == Init /\ [][Next]_vars

-----------------------------------------------------------------------------
(***************************************************************************)
(* INVARIANTS                                                              *)
(***************************************************************************)

TypeOK ==
    /\ epoch \in 1..MaxEpoch
    /\ activeSet \subseteq Validators
    /\ pendingSet \subseteq Validators
    /\ pendingPower \in [Validators -> 0..MaxPower]
    /\ gadgetSet \subseteq Validators
    /\ gadgetPower \in [Validators -> 0..MaxPower]
    /\ gadgetTotal \in Nat
    /\ power \in [Validators -> 0..MaxPower]
    /\ votes \subseteq Votes
    /\ counted \subseteq votes

(***************************************************************************)
(* QuorumThresholdSound. For every reachable validator set — including     *)
(* sizes 0, 1, 2 and 3 — the computed threshold must be the smallest       *)
(* weight that is strictly more than two thirds of total power.            *)
(*                                                                         *)
(* Two directions, both needed:                                            *)
(*   - the threshold IS a supermajority (not too low — a low threshold     *)
(*     lets a minority finalize)                                           *)
(*   - one less is NOT (not too high — a high threshold stalls the chain)  *)
(***************************************************************************)
QuorumThresholdSound ==
    LET total == RealTotal
        req   == RequiredWeight(total)
    IN /\ IsSupermajority(req, total)
       /\ ~ IsSupermajority(req - 1, total)

(***************************************************************************)
(* An empty validator set must not be able to finalize anything.           *)
(***************************************************************************)
EmptySetCannotFinalize ==
    (activeSet = {}) => ~ IsSupermajority(0, RealTotal)

(***************************************************************************)
(* NoStaleVoteCounted. A vote scoped to epoch E is never counted toward a  *)
(* quorum in any other epoch. Violated by design when EnforceVoteEpoch is  *)
(* FALSE — see ValidatorSet_UnscopedVotes.cfg (#2256).                     *)
(***************************************************************************)
NoStaleVoteCounted ==
    \A m \in counted : m.ep = epoch

(***************************************************************************)
(* Only current members contribute weight.                                 *)
(***************************************************************************)
OnlyMembersContributeWeight ==
    CountedWeight <= RealTotal

(***************************************************************************)
(* THE CENTRAL SAFETY PROPERTY, and the one the previous revision could    *)
(* not express because it had no Finalize action.                          *)
(*                                                                         *)
(* Every finalisation must have been backed by a strict supermajority of   *)
(* the stake that ACTUALLY EXISTED when it was taken — not of whatever     *)
(* total the gadget happened to be holding.                                *)
(*                                                                         *)
(* This is NOT vacuous. `Finalize`'s guard tests `GadgetTotal`; this       *)
(* invariant tests `realTotal`. They coincide only while the gadget's view *)
(* matches the chain. Under GadgetTracksSet = FALSE they diverge across a  *)
(* rotation, and TLC finalises a block on a minority of the real stake —   *)
(* see ValidatorSet_StaleGadget.cfg.                                       *)
(***************************************************************************)
FinalizedOnlyByRealSupermajority ==
    \A r \in finalRecs : IsSupermajority(r.backing, r.realTotal)

(***************************************************************************)
(* The pending set is only ever a staged intent: it must never authorise a *)
(* vote or contribute weight before activation.                           *)
(***************************************************************************)
PendingSetIsNotAuthoritative ==
    LET voters == { m.voter : m \in counted }
    IN SumPower(voters \cap activeSet) = CountedWeight

===============================================================================
