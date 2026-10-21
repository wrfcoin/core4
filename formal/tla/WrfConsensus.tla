---------------------------- MODULE WrfConsensus ----------------------------
(***************************************************************************)
(* TLA+ model of WRFCoin BFT block proposal, voting and finalization.      *)
(*                                                                         *)
(* Story #2098, epic #2093.                                                *)
(*                                                                         *)
(* DERIVED FROM, at commit b0bd2060c:                                      *)
(*   consensus-engines/src/selection/bft_finality.rs                       *)
(*       - BftFinalityGadget::is_supermajority  (line 511)                 *)
(*       - BftFinalityGadget::required_weight   (line 516)                 *)
(*       - BftFinalityGadget::submit_vote       (line 537)                 *)
(*         steps 2 (AlreadyFinalized), 3 (equivocation/duplicate),         *)
(*         4 (record), 5 (supermajority check)                             *)
(*   consensus-engines/src/engine.rs, validator.rs, algorithms/            *)
(*   shared-protocol/src/consensus/finality.rs (FinalitySignal delivery)   *)
(*                                                                         *)
(* THE PROPERTY THIS EXISTS TO CHECK                                       *)
(*                                                                         *)
(* Each node runs its own BftFinalityGadget and accumulates votes that     *)
(* reach it over p2p. Nodes therefore see different vote subsets. A        *)
(* Byzantine validator can exploit that by sending vote(A) to one node and *)
(* vote(B) to another: each node's local equivocation check (Rust step 3,  *)
(* the `validator_height_votes` map) sees only one of the two and is       *)
(* satisfied. Safety must come from quorum intersection, not from the      *)
(* local duplicate check.                                                  *)
(*                                                                         *)
(* NoConflictingFinalization is the assertion that this attack cannot      *)
(* succeed at f = 1, n = 4. See formal/README.md for the negative control  *)
(* that confirms the model can actually detect the attack when it should.  *)
(***************************************************************************)
EXTENDS Naturals, FiniteSets

CONSTANTS
    Validators,             \* set of validator identities
    ByzantineValidators,    \* subset of Validators that may equivocate
    Nodes,                  \* the observers whose finalized views must agree
    Blocks,                 \* candidate block hashes available at a height
    MaxHeight,              \* highest block height modelled
    HeavyValidators,        \* validators holding HeavyStake; others hold LightStake
    HeavyStake,             \* stake held by each heavy validator
    LightStake              \* stake held by every other validator

ASSUME ByzantineValidators \subseteq Validators
ASSUME MaxHeight \in Nat /\ MaxHeight >= 1
ASSUME Cardinality(Blocks) >= 1
ASSUME HeavyValidators \subseteq Validators
ASSUME HeavyStake \in Nat \ {0}
ASSUME LightStake \in Nat \ {0}

Honest == Validators \ ByzantineValidators

(***************************************************************************)
(* Stake weight.                                                           *)
(*                                                                         *)
(* This was `Weight(v) == 1` — uniform — which made every result           *)
(* validator-COUNT arithmetic. The Rust is stake-weighted: `bft_finality`  *)
(* sums `FinalityValidator.stake_weight` into `total_weight`, and adds each *)
(* voter's stake to `accumulated_weight` before applying                   *)
(* `accumulated * 3 > total * 2`. A safety result proved under uniform     *)
(* weights does not transfer: one validator holding a majority of stake    *)
(* behaves nothing like one validator holding one vote (#2255 review).     *)
(*                                                                         *)
(* So stake is now supplied per configuration, as a TWO-LEVEL distribution: *)
(* a set of heavy validators and one stake value for each level. TLC's .cfg *)
(* parser cannot express a function literal such as [v1 |-> 5, ...], and    *)
(* the model values v1..v4 are declared in the configuration rather than    *)
(* here, so an arbitrary per-validator function cannot be written in either *)
(* file. Two levels is not arbitrary stake, and this file does not pretend  *)
(* otherwise — but it is sufficient to separate COUNT from STAKE, which is  *)
(* the distinction the uniform model could not make at all.                 *)
(*                                                                          *)
(* The uniform configurations set HeavyStake = LightStake = 1 and are       *)
(* therefore unchanged in meaning; WrfConsensus_Weighted.cfg exercises      *)
(* non-uniform stake, and WrfConsensus_WeightedNegativeControl.cfg exhibits *)
(* a validator set that is safe by COUNT and unsafe by STAKE.               *)
(***************************************************************************)
Weight(v) == IF v \in HeavyValidators THEN HeavyStake ELSE LightStake

RECURSIVE SumWeights(_)
SumWeights(S) ==
    IF S = {} THEN 0
    ELSE LET v == CHOOSE x \in S : TRUE
         IN Weight(v) + SumWeights(S \ {v})

TotalWeight == SumWeights(Validators)

(***************************************************************************)
(* Mirrors bft_finality.rs exactly.                                        *)
(*                                                                         *)
(*   fn is_supermajority(accumulated, total) { accumulated * 3 > total * 2 }*)
(*   fn required_weight(&self) { (self.total_weight * 2 / 3) + 1 }         *)
(*                                                                         *)
(* Both forms are kept so RequiredWeightMatchesSupermajority can assert    *)
(* they agree — a divergence between them in the Rust would be a silent    *)
(* off-by-one in the finality threshold.                                   *)
(***************************************************************************)
IsSupermajority(accumulated) == accumulated * 3 > TotalWeight * 2
RequiredWeight == ((TotalWeight * 2) \div 3) + 1

(***************************************************************************)
(* A vote is a record. `inFlight` is the network soup; `counted` is each   *)
(* node's local BftFinalityGadget state.                                   *)
(***************************************************************************)
Votes == [voter : Validators, blk : Blocks]

VARIABLES
    height,        \* the height currently being voted on; MaxHeight+1 = done
    proposed,      \* SUBSET Blocks proposed at the current height
    cast,          \* SUBSET Votes: every vote CAST at this height, ever
    inFlight,      \* SUBSET cast: not yet delivered anywhere, and droppable
    counted,       \* [Nodes -> SUBSET Votes] each node's accepted votes
    finalizedAt    \* [1..MaxHeight -> SUBSET Blocks] blocks finalized per height

vars == <<height, proposed, cast, inFlight, counted, finalizedAt>>

(***************************************************************************)
(* `cast` and `inFlight` are deliberately separate.                        *)
(*                                                                         *)
(* An earlier revision used one `sent` set for both, and TLC immediately   *)
(* produced a false NoConflictingFinalization counterexample: DropMessage  *)
(* removed a vote from `sent`, which re-enabled the "an honest validator   *)
(* votes at most once" guard, and the honest validator equivocated. The    *)
(* model, not the protocol, was wrong. `cast` is monotone within a height  *)
(* and is what the vote guards read; `inFlight` is the droppable network   *)
(* soup. Losing a message must never grant a validator a second vote.      *)
(***************************************************************************)

(***************************************************************************)
(* Per-height state is cleared when a height closes. This mirrors Rust     *)
(* step 2: `if vote.block_height <= self.finalized_height { AlreadyFinalized }` *)
(* — a closed height accepts no further votes — and it is what keeps the   *)
(* state space finite as MaxHeight grows.                                  *)
(***************************************************************************)
Init ==
    /\ height = 1
    /\ proposed = {}
    /\ cast = {}
    /\ inFlight = {}
    /\ counted = [n \in Nodes |-> {}]
    /\ finalizedAt = [h \in 1..MaxHeight |-> {}]

Running == height <= MaxHeight

\* Votes a node has counted for a particular block.
VotersFor(n, b) == { m.voter : m \in { x \in counted[n] : x.blk = b } }
WeightFor(n, b) == SumWeights(VotersFor(n, b))

\* Has node n already counted a vote from this validator at this height?
HasCounted(n, v) == \E m \in counted[n] : m.voter = v

-----------------------------------------------------------------------------
(***************************************************************************)
(* ACTIONS                                                                 *)
(***************************************************************************)

\* An honest proposer puts forward a single block for the height.
Propose ==
    /\ Running
    /\ proposed = {}
    /\ \E b \in Blocks : proposed' = {b}
    /\ UNCHANGED <<height, cast, inFlight, counted, finalizedAt>>

\* A Byzantine proposer equivocates: a second, conflicting block at the same
\* height. Only reachable when the adversary actually controls a validator.
ByzantineEquivocate ==
    /\ Running
    /\ ByzantineValidators # {}
    /\ proposed # {}
    /\ \E b \in Blocks \ proposed : proposed' = proposed \cup {b}
    /\ UNCHANGED <<height, cast, inFlight, counted, finalizedAt>>

\* An honest validator votes once per height, for a proposed block.
Vote ==
    /\ Running
    /\ \E v \in Honest, b \in proposed :
        /\ ~ \E m \in cast : m.voter = v
        /\ cast' = cast \cup {[voter |-> v, blk |-> b]}
        /\ inFlight' = inFlight \cup {[voter |-> v, blk |-> b]}
    /\ UNCHANGED <<height, proposed, counted, finalizedAt>>

\* A Byzantine validator may vote for every proposed block at the same
\* height. This is the double-vote half of the equivocation attack: the
\* local check in each node's gadget cannot see the other node's copy.
ByzantineDoubleVote ==
    /\ Running
    /\ \E v \in ByzantineValidators, b \in proposed :
        /\ ~ \E m \in cast : m.voter = v /\ m.blk = b
        /\ cast' = cast \cup {[voter |-> v, blk |-> b]}
        /\ inFlight' = inFlight \cup {[voter |-> v, blk |-> b]}
    /\ UNCHANGED <<height, proposed, counted, finalizedAt>>

\* Delivery to one node. Mirrors Rust step 3: a node refuses a second vote
\* from a validator it has already counted at this height, so whichever copy
\* of an equivocating pair arrives first is the one that counts.
DeliverMessage ==
    /\ Running
    /\ \E n \in Nodes, m \in inFlight :
        /\ ~ HasCounted(n, m.voter)
        /\ counted' = [counted EXCEPT ![n] = counted[n] \cup {m}]
    /\ UNCHANGED <<height, proposed, cast, inFlight, finalizedAt>>

\* The network loses a message outright. A vote that has already been
\* counted somewhere is unaffected; one that has not can be lost forever.
DropMessage ==
    /\ Running
    /\ \E m \in inFlight :
        inFlight' = inFlight \ {m}
    /\ UNCHANGED <<height, proposed, cast, counted, finalizedAt>>

\* Redelivery of a vote a node has already counted. Deliberately a no-op:
\* that IS the property — the gadget's DuplicateVoteRejected path means
\* redelivery cannot change any node's tally. Kept as an explicit action so
\* the model states the guarantee rather than omitting the case.
DuplicateMessage ==
    /\ Running
    /\ \E n \in Nodes : \E m \in counted[n] :
        counted' = [counted EXCEPT ![n] = counted[n] \cup {m}]
    /\ UNCHANGED <<height, proposed, cast, inFlight, finalizedAt>>

\* A node observes a supermajority and finalizes. finalizedAt accumulates
\* across nodes, so two nodes finalizing different blocks at one height is
\* visible as a set of size 2 — exactly NoConflictingFinalization.
Finalize ==
    /\ Running
    /\ \E n \in Nodes, b \in proposed :
        /\ IsSupermajority(WeightFor(n, b))
        /\ finalizedAt' = [finalizedAt EXCEPT ![height] = finalizedAt[height] \cup {b}]
    /\ UNCHANGED <<height, proposed, cast, inFlight, counted>>

\* Close the height and move on, discarding per-height vote state. Covers
\* both the happy path (something finalized) and a view-change timeout
\* (nothing finalized) — the latter is what makes liveness non-trivial.
Timeout ==
    /\ Running
    /\ height' = height + 1
    /\ proposed' = {}
    /\ cast' = {}
    /\ inFlight' = {}
    /\ counted' = [n \in Nodes |-> {}]
    /\ UNCHANGED finalizedAt

\* Terminal stutter so TLC does not report deadlock once MaxHeight is passed.
Done ==
    /\ height > MaxHeight
    /\ UNCHANGED vars

Next ==
    \/ Propose
    \/ ByzantineEquivocate
    \/ Vote
    \/ ByzantineDoubleVote
    \/ DeliverMessage
    \/ DropMessage
    \/ DuplicateMessage
    \/ Finalize
    \/ Timeout
    \/ Done

Spec == Init /\ [][Next]_vars

\* Fairness for the liveness check only. Dropping is deliberately left
\* unfair — a network that drops every message forever cannot finalize, and
\* asserting otherwise would be false.
FairSpec ==
    /\ Spec
    /\ WF_vars(Propose)
    /\ WF_vars(Vote)
    /\ WF_vars(DeliverMessage)
    /\ WF_vars(Finalize)

(***************************************************************************)
(* LIVENESS UNDER PARTIAL SYNCHRONY                                        *)
(*                                                                         *)
(* Liveness cannot be stated against `Next` as written. `Timeout` is always *)
(* enabled, so "time out at height 1 before anything happens, forever" is a *)
(* fair behaviour that trivially violates EventuallyFinalizes. That is an   *)
(* artefact of modelling a view timer with no lower bound, not a protocol   *)
(* defect: no BFT protocol claims liveness under full asynchrony (FLP), and *)
(* real liveness arguments assume partial synchrony — the view timer        *)
(* eventually exceeds the message delay.                                    *)
(*                                                                         *)
(* LiveNext encodes exactly those two assumptions:                          *)
(*   1. A height is only closed once it has decided (the timer is long      *)
(*      enough not to pre-empt a reachable quorum).                         *)
(*   2. DropMessage is excluded (delivery is eventually reliable).          *)
(*                                                                         *)
(* See formal/README.md for what TLC reports against this, and why the      *)
(* remaining counterexample is informative rather than alarming.            *)
(***************************************************************************)
LiveNext ==
    \/ Propose
    \/ ByzantineEquivocate
    \/ Vote
    \/ ByzantineDoubleVote
    \/ DeliverMessage
    \/ DuplicateMessage
    \/ Finalize
    \/ (Timeout /\ finalizedAt[height] # {})
    \/ Done

LiveSpec ==
    /\ Init
    /\ [][LiveNext]_vars
    /\ WF_vars(Propose)
    /\ WF_vars(Vote)
    /\ WF_vars(DeliverMessage)
    /\ WF_vars(Finalize)

-----------------------------------------------------------------------------
(***************************************************************************)
(* TYPE INVARIANT                                                          *)
(***************************************************************************)
TypeOK ==
    /\ height \in 1..(MaxHeight + 1)
    /\ proposed \subseteq Blocks
    /\ cast \subseteq Votes
    /\ inFlight \subseteq cast
    /\ counted \in [Nodes -> SUBSET Votes]
    /\ finalizedAt \in [1..MaxHeight -> SUBSET Blocks]

-----------------------------------------------------------------------------
(***************************************************************************)
(* SAFETY                                                                  *)
(***************************************************************************)

(***************************************************************************)
(* INV-FINALITY-1. No two nodes finalize different blocks at one height.   *)
(* This is the Rust invariant in shared-protocol/src/invariants/finality.rs *)
(* (#2096) stated at the protocol level rather than the process level.     *)
(***************************************************************************)
NoConflictingFinalization ==
    \A h \in 1..MaxHeight : Cardinality(finalizedAt[h]) <= 1

(***************************************************************************)
(* Any two quorums share an honest validator. This is *why*                *)
(* NoConflictingFinalization holds; checking it separately means a         *)
(* threshold change that breaks the reason shows up even in a run where    *)
(* the adversary happened not to exploit it.                               *)
(*                                                                         *)
(* State-independent — a property of the validator set and the threshold.  *)
(***************************************************************************)
Quorums == { Q \in SUBSET Validators : IsSupermajority(SumWeights(Q)) }

QuorumIntersection ==
    \A Q1 \in Quorums, Q2 \in Quorums :
        (Q1 \cap Q2) \cap Honest # {}

(***************************************************************************)
(* The two threshold formulations in bft_finality.rs must agree. If        *)
(* `required_weight()` ever drifts from `is_supermajority()`, certificate  *)
(* validation and vote accumulation would disagree about what finality is. *)
(***************************************************************************)
RequiredWeightMatchesSupermajority ==
    /\ IsSupermajority(RequiredWeight)
    /\ ~ IsSupermajority(RequiredWeight - 1)

(***************************************************************************)
(* The Byzantine bound, expressed BY WEIGHT rather than by count (#2255).  *)
(*                                                                         *)
(* A 2/3 threshold tolerates Byzantine power strictly below 1/3 of the     *)
(* total. Under uniform stake that is a statement about how MANY           *)
(* validators are faulty; under real stake it is a statement about how     *)
(* MUCH STAKE is faulty, and the two come apart badly — one validator out  *)
(* of four is within the count bound while holding half the stake is far   *)
(* outside the weight bound.                                               *)
(*                                                                         *)
(* Stated as a checkable invariant rather than an ASSUME on purpose: an    *)
(* ASSUME would make a violating configuration fail to start, which looks  *)
(* like a broken model. As an invariant, a configuration that exceeds the  *)
(* bound RUNS and reports precisely which property the excess breaks —     *)
(* which is what WrfConsensus_WeightedNegativeControl.cfg demonstrates.    *)
(***************************************************************************)
ByzantineWeight == SumWeights(ByzantineValidators)

ByzantineWeightWithinBound == ByzantineWeight * 3 < TotalWeight

(***************************************************************************)
(* Finalized entries are block identities, not junk.                       *)
(*                                                                         *)
(* Renamed from `OnlyProposedBlocksFinalize` (#2255 review): that name      *)
(* claimed more than the predicate checks. It asserts membership of        *)
(* `Blocks` — the set of candidate identities — NOT that the block was     *)
(* proposed at that height. Checking the stronger property would need a    *)
(* per-height record of what was proposed; `proposed` holds only the       *)
(* CURRENT height and is cleared when the height advances, so the history  *)
(* does not exist to check against.                                        *)
(*                                                                         *)
(* Left as a type-level sanity check under an accurate name rather than    *)
(* strengthened, because the safety argument rests on                      *)
(* NoConflictingFinalization and QuorumIntersection, not on this. Adding   *)
(* the history is tracked as follow-up rather than done here.              *)
(***************************************************************************)
FinalizedBlocksAreKnownBlocks ==
    \A h \in 1..MaxHeight : finalizedAt[h] \subseteq Blocks

-----------------------------------------------------------------------------
(***************************************************************************)
(* LIVENESS — checked separately, under FairSpec. See formal/README.md.    *)
(***************************************************************************)
EventuallyFinalizes == <>(finalizedAt[1] # {})

=============================================================================
