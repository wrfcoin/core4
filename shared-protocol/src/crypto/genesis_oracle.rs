//! The oracle set, declared in genesis metadata (#2523).
//!
//! # Why this exists
//!
//! The oracle set is genesis content. #2488 established that every block-1
//! `OracleRegistration` must carry the *final* set and quorum, because block
//! transaction order is not stable and an incremental payload can seat one
//! oracle behind a quorum that can never be met.
//!
//! That rule compares each registration against **its siblings in the same
//! block**. It is the strongest check available from the block alone, and it
//! has a hole it cannot close: a set on which every registration *agrees* and
//! which is nonetheless *wrong* is accepted unanimously. There is no absolute
//! reference to compare against — so the rule can enforce consistency but not
//! correctness.
//!
//! This module supplies the missing reference. Genesis declares the intended
//! set and quorum; block-1 registrations are validated against **that**, not
//! merely against each other.
//!
//! The philosophy is already stated in this crate for protocol version, and is
//! adopted verbatim here: **genesis decides, local configuration may only
//! agree.** A node does not get to choose the oracle set any more than it gets
//! to choose the protocol version.
//!
//! # Encoding
//!
//! Block metadata is a string map, so the set is encoded as **sorted, deduped,
//! lowercase hex joined by `,`**. Sorting and deduping happen at encode time so
//! the encoded form is canonical: two nodes that declare the same set produce
//! byte-identical metadata, and therefore the same `state_root`. A set that
//! arrives unsorted is not rejected — it is normalised — because the ceremony
//! writes this by hand and ordering is not a meaningful difference.

use crate::crypto::signature::PublicKey;

/// Key under which the genesis block records the intended oracle set (#2523).
pub const GENESIS_ORACLE_SET_KEY: &str = "oracle_set";

/// Key under which the genesis block records the intended oracle quorum (#2523).
pub const GENESIS_ORACLE_QUORUM_KEY: &str = "oracle_quorum";

/// The `oracle_set` value that declares **"this chain has no oracles"**.
///
/// # Why silence cannot mean this
///
/// A block 1 with no declaration and a block 1 declaring `none` produce the same
/// chain — no oracle set, permanently. They are not the same *statement*. The
/// first is an operator who configured nothing; the second is an operator who
/// decided. Collapsing them puts the dangerous state on the unenforced branch,
/// which is precisely the collapse this module already refuses for "declared and
/// wrong" vs "not declared" (see [`GenesisOracleError::EmptyOracleSet`]).
///
/// So the posture is mandatory at height 1 and this is how a dev, test, or
/// legacy-shaped chain states it. It is unambiguous against a real set: every
/// entry in a real set must parse as ed25519 public-key hex, which `none` does
/// not.
pub const GENESIS_ORACLE_NONE: &str = "none";

/// What genesis says about oracles — the two legal postures.
///
/// There is deliberately no third variant for "said nothing". Absence is
/// represented by `Ok(None)` from [`genesis_oracle_declaration`] and is a hard
/// refusal at height 1; see `ProofOfPublishing::check_block_one_genesis_oracles`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenesisOraclePosture {
    /// Explicit: this chain has no oracle set. Block 1 must seat none.
    NoOracles,
    /// A canonical set and quorum that block 1 must reproduce exactly.
    Declared(GenesisOracleDeclaration),
}

/// Environment variable naming the oracle set for a **fresh cut** (#2523).
///
/// # Read exactly once, by exactly one node, and never at validation
///
/// This was `WRFCOIN_GENESIS_ORACLE_SET` in its previous life as a *validation
/// reference* (#2496), and Bob refuted that use of it: a node with the variable
/// unset reached a branch that validated nothing and emitted no diagnostic, so
/// one forgotten export on cut day meant that node accepted a unanimous-but-wrong
/// block 1 while correctly-configured nodes refused it — a partition at best.
///
/// The variable survives; its *role* does not. It is now consumed only by
/// [`cut_oracle_declaration`], on the single node producing block 1, and its
/// value is written straight into that block's metadata. From that instant the
/// canonical set is chain content and every node — configured, misconfigured, or
/// not configured at all — derives the identical answer from the block.
///
/// Format is unchanged: `{"oracle_set":["<hex>",...],"oracle_quorum":K}`, or the
/// literal [`GENESIS_ORACLE_NONE`] to cut a chain with no oracles. N and K are
/// hardcoded nowhere, so the owner decision (N=3, K=2) changes only this value.
pub const CUT_ORACLE_SET_ENV: &str = "WRFCOIN_GENESIS_ORACLE_SET";

#[derive(serde::Deserialize)]
struct CutOracleSetDoc {
    oracle_set: Vec<String>,
    oracle_quorum: usize,
}

/// Parse the cut-time oracle declaration into its canonical metadata encoding.
///
/// Returns `Ok(None)` when the operator configured nothing — the caller decides
/// what that means, and on the cut path
/// `ProofOfPublishing::stamp_genesis_oracle_declaration` refuses. Returns
/// `(oracle_set, oracle_quorum)` ready to be injected via
/// `ProofOfPublishing::set_genesis_oracle_declaration`.
///
/// Takes the raw value rather than reading the environment, for the same reason
/// `genesis_producers::resolve_from_values` does: this is startup configuration
/// with consensus consequences, and it must be unit-testable without mutating
/// process-global state. Nothing in the consensus path reads the environment for
/// the oracle set — that was the #2496 defect, not an implementation detail.
pub fn parse_cut_oracle_declaration(raw: Option<&str>) -> Result<Option<(String, String)>, String> {
    let raw = raw.map(str::trim).filter(|v| !v.is_empty());

    let Some(raw) = raw else {
        return Ok(None);
    };

    if raw.eq_ignore_ascii_case(GENESIS_ORACLE_NONE) {
        return Ok(Some((GENESIS_ORACLE_NONE.to_string(), "0".to_string())));
    }

    let doc: CutOracleSetDoc = serde_json::from_str(raw).map_err(|e| {
        format!(
            "refusing to cut block 1: {CUT_ORACLE_SET_ENV} is not valid JSON of the form \
             {{\"oracle_set\":[..],\"oracle_quorum\":K}}: {e}. Refusing rather than guessing — a \
             misread ceremony artifact seats the wrong oracle set into state_root at genesis, on \
             a block that cannot be re-cut (#2523)."
        )
    })?;

    // Reuse the declaration's own admission rules so the cut cannot write a
    // declaration the chain's validator would then disown.
    let declaration = GenesisOracleDeclaration::new(doc.oracle_set, doc.oracle_quorum)
        .map_err(|e| format!("refusing to cut block 1: {CUT_ORACLE_SET_ENV} is invalid: {e}"))?;

    Ok(Some((
        declaration.encode_oracle_set(),
        declaration.oracle_quorum().to_string(),
    )))
}

/// [`parse_cut_oracle_declaration`] against the process environment — the single
/// env read for the oracle set in the entire codebase, called once by the node at
/// startup, outside any consensus path.
pub fn cut_oracle_declaration_from_env() -> Result<Option<(String, String)>, String> {
    // determinism-ok: startup-only cut configuration, parsed before block processing begins and committed into block 1's metadata; never consulted during validation.
    let raw = std::env::var(CUT_ORACLE_SET_ENV).ok();
    parse_cut_oracle_declaration(raw.as_deref())
}

/// What genesis says the oracle set and quorum must be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenesisOracleDeclaration {
    /// Sorted, deduped, lowercase-hex oracle identities.
    oracle_set: Vec<String>,
    oracle_quorum: usize,
}

impl GenesisOracleDeclaration {
    /// Build a declaration, normalising and validating the set.
    pub fn new(
        oracle_set: Vec<String>,
        oracle_quorum: usize,
    ) -> Result<Self, GenesisOracleError> {
        let mut normalized = Vec::with_capacity(oracle_set.len());
        for raw in &oracle_set {
            let trimmed = raw.trim();
            // Same admission rule as `State::validate_oracle_config`, so the
            // declaration cannot describe a set the chain would refuse to
            // install. A friendly operator name here is a cut-day abort.
            if PublicKey::from_hex(trimmed).is_err() {
                return Err(GenesisOracleError::InvalidOracleId {
                    raw: trimmed.to_string(),
                });
            }
            normalized.push(trimmed.to_ascii_lowercase());
        }
        normalized.sort();
        normalized.dedup();

        if normalized.is_empty() {
            return Err(GenesisOracleError::EmptyOracleSet);
        }
        if oracle_quorum == 0 {
            return Err(GenesisOracleError::QuorumOutOfRange {
                quorum: oracle_quorum,
                set_len: normalized.len(),
            });
        }
        if oracle_quorum > normalized.len() {
            return Err(GenesisOracleError::QuorumOutOfRange {
                quorum: oracle_quorum,
                set_len: normalized.len(),
            });
        }

        Ok(Self {
            oracle_set: normalized,
            oracle_quorum,
        })
    }

    /// The declared set, sorted and deduped.
    pub fn oracle_set(&self) -> &[String] {
        &self.oracle_set
    }

    /// The declared quorum.
    pub fn oracle_quorum(&self) -> usize {
        self.oracle_quorum
    }

    /// The canonical metadata encoding of the set.
    pub fn encode_oracle_set(&self) -> String {
        self.oracle_set.join(",")
    }

    /// Does a registration's payload carry exactly what genesis declared?
    ///
    /// The payload's set is normalised the same way before comparison, so a
    /// registration that lists the right oracles in a different order matches.
    /// The quorum is compared exactly — it is a number, not a set.
    pub fn matches(&self, payload_set: &[String], payload_quorum: usize) -> bool {
        if payload_quorum != self.oracle_quorum {
            return false;
        }
        let mut normalized: Vec<String> = payload_set
            .iter()
            .map(|s| s.trim().to_ascii_lowercase())
            .collect();
        normalized.sort();
        normalized.dedup();
        normalized == self.oracle_set
    }
}

/// Validate a block's oracle registrations against the set that block declares.
///
/// # Why this is block-level rather than per-transaction
///
/// #2488's sibling-agreement rule lives in `classify_block_transaction`, which
/// the producer self-screen runs *before* block metadata is written — so a
/// per-transaction check against the declaration would be invisible to the
/// producer and enforced only by its peers. That is the producer/sync
/// validation asymmetry #1730 closed, and it is not worth reopening. As a
/// block-level invariant, the producer runs the identical check on its
/// assembled block and reaches the same verdict as every verifier.
///
/// Everything needed is in the block: the declaration in its metadata, the
/// registrations in its transactions. Checkable from the block alone, which is
/// the property #2488 was built for.
///
/// # Conformance only — the seating rules are height-1 and live next door
///
/// This answers "does every registration agree with what genesis declared?",
/// which is true at every height: a mid-chain registration naming a different
/// set contradicts genesis just as surely as a block-1 one does. Whether the
/// block actually *installs* the declared set is a separate, genesis-only
/// question — see [`validate_block_one_seats_oracles`]. Folding the two together
/// made a height-2 block carrying a declaration and no registrations a chain
/// halt.
///
/// `Ok(())` when genesis declares nothing — every chain cut before #2523.
pub fn validate_block_oracle_registrations<'a, M, R>(
    metadata: M,
    registrations: R,
) -> Result<(), GenesisOracleError>
where
    M: IntoIterator<Item = (&'a str, &'a str)>,
    R: IntoIterator<Item = (Vec<String>, usize, String)>,
{
    let declaration = match genesis_oracle_declaration(metadata)? {
        Some(GenesisOraclePosture::Declared(d)) => d,
        // A chain that declared it has no oracles must seat none. A registration
        // on such a block contradicts genesis just as surely as a wrong set
        // does, and genesis is what decides.
        Some(GenesisOraclePosture::NoOracles) => {
            let mut offender = None;
            for (payload_set, payload_quorum, _) in registrations {
                if payload_set.iter().any(|e| !e.trim().is_empty()) {
                    offender = Some((payload_set, payload_quorum));
                    break;
                }
            }
            return match offender {
                None => Ok(()),
                Some((got_set, got_quorum)) => {
                    let mut got: Vec<String> =
                        got_set.iter().map(|s| s.trim().to_ascii_lowercase()).collect();
                    got.sort();
                    got.dedup();
                    Err(GenesisOracleError::RegistrationDisagreesWithGenesis {
                        declared_set: Vec::new(),
                        declared_quorum: 0,
                        got_set: got,
                        got_quorum,
                    })
                },
            };
        },
        None => return Ok(()),
    };

    for (payload_set, payload_quorum, _identity) in registrations {
        if !declaration.matches(&payload_set, payload_quorum) {
            let mut got: Vec<String> = payload_set
                .iter()
                .map(|s| s.trim().to_ascii_lowercase())
                .collect();
            got.sort();
            got.dedup();
            return Err(GenesisOracleError::RegistrationDisagreesWithGenesis {
                declared_set: declaration.oracle_set.clone(),
                declared_quorum: declaration.oracle_quorum,
                got_set: got,
                got_quorum: payload_quorum,
            });
        }
    }

    Ok(())
}

/// The SEATING half of the genesis rule, applied at height 1 only.
///
/// Split out from [`validate_block_oracle_registrations`] deliberately. That
/// function answers "does each registration agree with genesis?", which is true
/// at every height — a mid-chain re-registration naming a different set is just
/// as wrong. These two questions are only meaningful at genesis:
///
/// * a declared set that the block seats **none** of — the declaration is
///   decorative and the chain comes up empty behind the declared quorum;
/// * a declared set the block seats only **part** of — some operator's key never
///   got registered, so the set comes up short behind that quorum.
///
/// Above height 1 both are recoverable in a later block and enforcing them would
/// halt production, which is how the first version of this rule turned a
/// height-1 refusal into a height-2 chain stop (caught by
/// `an_undeclared_block_above_height_one_is_unaffected`).
fn validate_block_one_seats_the_declared_set(
    declaration: &GenesisOracleDeclaration,
    seated: &[String],
    any_registration: bool,
) -> Result<(), GenesisOracleError> {
    if !any_registration {
        return Err(GenesisOracleError::DeclaredButUnseated {
            declared_set: declaration.oracle_set.clone(),
            declared_quorum: declaration.oracle_quorum,
        });
    }
    // Each operator registers its OWN key, so the registered identities must
    // cover the declared set. Counting registrations is not enough: three
    // registrations from the same operator would pass a count check and still
    // leave two declared oracles unseated behind the quorum.
    let mut seated: Vec<String> = seated.to_vec();
    seated.sort();
    seated.dedup();
    let missing: Vec<String> = declaration
        .oracle_set
        .iter()
        .filter(|declared| !seated.contains(declared))
        .cloned()
        .collect();
    if !missing.is_empty() {
        return Err(GenesisOracleError::IncompletelySeated {
            declared: declaration.oracle_set.len(),
            seated: seated.len(),
            missing,
        });
    }
    Ok(())
}


/// Block 1 must actually install an oracle set — the OUTCOME gate (#2523).
///
/// # Why this is separate from the declaration check
///
/// [`validate_block_oracle_registrations`] only bites when genesis *declares* a
/// set. This one is unconditional at height 1, because the failure it prevents
/// does not require a declaration to exist.
///
/// # Why it must be phrased on the outcome, not the mempool
///
/// There are two routes to the identical dead chain, and only an outcome check
/// catches both:
///
/// | route | how block 1 loses the set | what the operator sees |
/// |---|---|---|
/// | nothing submitted | empty mempool | nothing; no confirmation |
/// | submitted then dropped | admitted to the mempool, removed at assembly by the submission-timing filter | **HTTP 201 `{"accepted":3}`** — a success receipt |
///
/// Both end at `{"oracle_set": [], "oracle_quorum": 2}` — a quorum of two over
/// zero oracles, on a block that cannot be re-cut. The second is worse because
/// the operator holds positive confirmation.
///
/// A gate phrased as *"the mempool must be non-empty"* or *"block 1 must carry
/// more than a coinbase"* catches the first and **misses the second**: assembly
/// could drop the registrations, include some unrelated transaction, and the
/// gate would pass while the chain still comes up dead. So this asks the only
/// question that distinguishes them — **after assembly and filtering, does this
/// block install a non-empty oracle set?**
///
/// Equivalently: the condition [`State::validate_oracle_config`] already
/// rejects, applied at height 1. A chain must not boot holding a configuration
/// its own validator disowns.
///
/// Heights above 1 are unaffected — a registration dropped mid-chain is
/// recoverable in a later block and must not halt production.
///
/// `metadata` is genesis's posture. A chain that explicitly declared
/// [`GENESIS_ORACLE_NONE`] seats nothing *by decision*, so the outcome gate does
/// not fire on it — the gate exists to catch a chain that meant to have oracles
/// and lost them, not one that said it would not.
pub fn validate_block_one_seats_oracles<'a, M, R>(
    height: u64,
    metadata: M,
    registrations: R,
) -> Result<(), GenesisOracleError>
where
    M: IntoIterator<Item = (&'a str, &'a str)>,
    R: IntoIterator<Item = (Vec<String>, usize, String)>,
{
    if height != 1 {
        return Ok(());
    }
    // Read the metadata twice: once for the posture, once for the network id.
    let meta: Vec<(&str, &str)> = metadata.into_iter().collect();
    let posture = genesis_oracle_declaration(meta.iter().copied())?;
    if matches!(posture, Some(GenesisOraclePosture::NoOracles)) {
        // #2523, Frank's review of #2540: `none` is NOT a free choice on the
        // public testnet.
        //
        // The escape hatch was documented as uncatchable — an operator who
        // wants oracles but writes `none` gets a permanently oracle-less chain,
        // and that was called a correctly-formed wrong decision that only the
        // ceremony checklist could mitigate. That was wrong. Block 1 already
        // carries `network_id`, so on the one network where the answer is known
        // in advance the block can be refused instead of the checklist being
        // trusted, and it costs no hardcoded N or K.
        //
        // `none` stays legal everywhere else, which is the whole point: dev and
        // local chains genuinely have no ceremony. Only the public testnet has a
        // declared expectation to contradict.
        let network_id = meta
            .iter()
            .find(|(k, _)| *k == crate::crypto::chain_binding::GENESIS_NETWORK_ID_KEY)
            .map(|(_, v)| *v);
        if crate::crypto::chain_binding::network_is_public_testnet(network_id) {
            return Err(GenesisOracleError::PublicTestnetDeclaredNoOracles);
        }
        return Ok(());
    }

    let mut seated: Vec<String> = Vec::new();
    let mut quorum: Option<usize> = None;
    let mut any_registration = false;
    for (payload_set, payload_quorum, identity) in registrations {
        any_registration = true;
        if payload_set.iter().any(|e| !e.trim().is_empty()) {
            seated.push(identity.trim().to_ascii_lowercase());
            quorum = Some(payload_quorum);
        }
    }

    seated.sort();
    seated.dedup();

    // When genesis NAMED a set, its rules run first — they diagnose the same
    // failures more precisely. A block seating one of three declared oracles is
    // `IncompletelySeated` ("these two never registered"), which tells the
    // operator what to fix; the generic `QuorumOutOfRange` below reports the
    // symptom (quorum 2 over 1 seated) and sends them to the wrong artifact.
    if let Some(GenesisOraclePosture::Declared(declaration)) = posture.as_ref() {
        validate_block_one_seats_the_declared_set(declaration, &seated, any_registration)?;
    }

    if seated.is_empty() {
        return Err(GenesisOracleError::BlockOneSeatsNoOracles);
    }

    // The set it would install must itself be one the chain would accept —
    // a quorum above the seated count is the unmeetable shape all over again.
    if let Some(q) = quorum {
        if q == 0 || q > seated.len() {
            return Err(GenesisOracleError::QuorumOutOfRange {
                quorum: q,
                set_len: seated.len(),
            });
        }
    }
    Ok(())
}

/// Why a genesis oracle declaration was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenesisOracleError {
    /// A registration carries a set or quorum genesis did not declare. This is
    /// the hole #2488 could not close: unanimous agreement on a wrong set.
    RegistrationDisagreesWithGenesis {
        declared_set: Vec<String>,
        declared_quorum: usize,
        got_set: Vec<String>,
        got_quorum: usize,
    },
    /// Genesis declares an oracle set and the block seats none of it.
    DeclaredButUnseated {
        declared_set: Vec<String>,
        declared_quorum: usize,
    },
    /// Block 1 installs no oracle set at all — after assembly and filtering, not
    /// merely an empty mempool. The chain would come up with a quorum over zero
    /// oracles, on a block that cannot be re-cut (#2523).
    BlockOneSeatsNoOracles,
    /// Genesis identifies the public testnet AND declares the `none` posture.
    /// Legal on any other network; refused here because the public testnet has a
    /// declared oracle expectation the block would silently contradict (#2523).
    PublicTestnetDeclaredNoOracles,
    /// Fewer registrations than declared oracles — some operator's key never
    /// got registered, so the set comes up short behind the declared quorum.
    IncompletelySeated {
        declared: usize,
        seated: usize,
        /// Declared oracles that no registration seated.
        missing: Vec<String>,
    },
    /// An entry is not ed25519 public-key hex.
    InvalidOracleId { raw: String },
    /// The declared set is empty. A chain declaring no oracles should omit the
    /// keys entirely rather than declare emptiness.
    EmptyOracleSet,
    /// Quorum is zero, or exceeds the set size — unmeetable either way.
    QuorumOutOfRange { quorum: usize, set_len: usize },
    /// The quorum value is not a base-10 integer.
    MalformedQuorum { raw: String },
    /// One key is present without the other. A set without a quorum, or a
    /// quorum without a set, is a half-written ceremony record and is refused
    /// rather than half-applied.
    IncompleteDeclaration { present: &'static str },
}

impl std::fmt::Display for GenesisOracleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidOracleId { raw } => write!(
                f,
                "genesis oracle_set entry {raw:?} is not an ed25519 public key hex string"
            ),
            Self::EmptyOracleSet => write!(
                f,
                "genesis declares an empty oracle_set; omit the keys entirely instead"
            ),
            Self::QuorumOutOfRange { quorum, set_len } => write!(
                f,
                "genesis oracle_quorum {quorum} is unmeetable against a set of {set_len}"
            ),
            Self::MalformedQuorum { raw } => {
                write!(f, "genesis oracle_quorum {raw:?} is not an integer")
            },
            Self::RegistrationDisagreesWithGenesis {
                declared_set,
                declared_quorum,
                got_set,
                got_quorum,
            } => write!(
                f,
                "oracle registration disagrees with the set genesis declared: \
                 got ({got_set:?}, {got_quorum}) but genesis declares \
                 ({declared_set:?}, {declared_quorum}). Genesis decides the oracle \
                 set; a block may only seat what genesis declared (#2523)"
            ),
            Self::DeclaredButUnseated {
                declared_set,
                declared_quorum,
            } => write!(
                f,
                "genesis declares an oracle set of {} at quorum {declared_quorum} but \
                 this block seats none of it — the chain would come up with an empty \
                 set behind an unmeetable quorum (#2523). declared: {declared_set:?}",
                declared_set.len()
            ),
            Self::IncompletelySeated {
                declared,
                seated,
                missing,
            } => write!(
                f,
                "genesis declares {declared} oracles but only {seated} distinct \
                 identit(ies) registered; {missing:?} never seated, so the set \
                 comes up short behind the declared quorum (#2523)"
            ),
            Self::BlockOneSeatsNoOracles => write!(
                f,
                "block 1 installs no oracle set: after assembly and filtering it carries \
                 no oracle registration. The chain would come up with a quorum over zero \
                 oracles and no later block can seat one without another cut (#2523). If \
                 registrations were submitted, they were dropped at assembly — check \
                 pop_skipped_mempool_transactions"
            ),
            Self::PublicTestnetDeclaredNoOracles => write!(
                f,
                "refusing to cut block 1: genesis declares network_id=testnet but \
                 oracle_set=none, which would bring up the public testnet \
                 permanently without oracles on a block that cannot be re-cut. \
                 `none` is the explicit no-ceremony posture and stays valid on dev \
                 and local chains; on the public testnet it is indistinguishable \
                 from a forgotten WRFCOIN_GENESIS_ORACLE_SET export. Set the \
                 ceremony's oracle set (runbook step 6b) or cut a chain whose \
                 network_id is not `testnet` (#2523)"
            ),
            Self::IncompleteDeclaration { present } => write!(
                f,
                "genesis declares {present} without its counterpart; both \
                 oracle_set and oracle_quorum are required together"
            ),
        }
    }
}

impl std::error::Error for GenesisOracleError {}

/// Read the oracle posture out of genesis metadata.
///
/// Mirrors [`crate::crypto::chain_binding::genesis_protocol_version`] in shape:
/// `Ok(None)` means genesis *said nothing*, `Err` means genesis said something
/// that cannot be honoured and the caller must fail closed rather than guess.
///
/// The two differ in what the caller may do with `Ok(None)`. A missing protocol
/// version has a safe default (V1). A missing oracle posture does not — the
/// chain simply has no oracle set, forever — so at height 1 the caller must
/// refuse rather than default. See
/// `ProofOfPublishing::check_block_one_genesis_oracles`.
pub fn genesis_oracle_declaration<'a, I>(
    metadata: I,
) -> Result<Option<GenesisOraclePosture>, GenesisOracleError>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut raw_set: Option<&str> = None;
    let mut raw_quorum: Option<&str> = None;
    for (key, value) in metadata {
        if key == GENESIS_ORACLE_SET_KEY {
            raw_set = Some(value);
        } else if key == GENESIS_ORACLE_QUORUM_KEY {
            raw_quorum = Some(value);
        }
    }

    match (raw_set, raw_quorum) {
        (None, None) => Ok(None),
        (Some(_), None) => Err(GenesisOracleError::IncompleteDeclaration {
            present: "oracle_set",
        }),
        (None, Some(_)) => Err(GenesisOracleError::IncompleteDeclaration {
            present: "oracle_quorum",
        }),
        (Some(set), Some(quorum)) => {
            let parsed_quorum = quorum.trim().parse::<usize>().map_err(|_| {
                GenesisOracleError::MalformedQuorum {
                    raw: quorum.to_string(),
                }
            })?;
            // The explicit "no oracles" posture. Its quorum must be 0: a chain
            // with no oracles behind a quorum of two is the unmeetable shape
            // this whole module exists to prevent, and writing it would be a
            // half-stated posture rather than a decision.
            if set.trim().eq_ignore_ascii_case(GENESIS_ORACLE_NONE) {
                if parsed_quorum != 0 {
                    return Err(GenesisOracleError::QuorumOutOfRange {
                        quorum: parsed_quorum,
                        set_len: 0,
                    });
                }
                return Ok(Some(GenesisOraclePosture::NoOracles));
            }
            let entries: Vec<String> = set
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if entries.is_empty() {
                return Err(GenesisOracleError::EmptyOracleSet);
            }
            GenesisOracleDeclaration::new(entries, parsed_quorum)
                .map(|d| Some(GenesisOraclePosture::Declared(d)))
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::chain_binding::GENESIS_NETWORK_ID_KEY;
    use crate::crypto::signature::PrivateKey;
    use rand::{SeedableRng, rngs::StdRng};

    fn key(seed: u64) -> String {
        PublicKey::from_private_key(&PrivateKey::generate(&mut StdRng::seed_from_u64(seed))).to_hex()
    }

    /// N=3, K=2 — the decided genesis shape.
    fn decided() -> (Vec<String>, usize) {
        (vec![key(2523_1), key(2523_2), key(2523_3)], 2)
    }

    fn meta<'a>(set: &'a str, quorum: &'a str) -> Vec<(&'a str, &'a str)> {
        vec![
            ("chain_id", "wrfcoin-testnet"),
            (GENESIS_ORACLE_SET_KEY, set),
            (GENESIS_ORACLE_QUORUM_KEY, quorum),
        ]
    }

    fn regs(set: &[String], quorum: usize) -> Vec<(Vec<String>, usize, String)> {
        set.iter()
            .map(|id| (set.to_vec(), quorum, id.clone()))
            .collect()
    }

    /// Metadata for a chain that DECLARES it intends oracles. The outcome-gate
    /// tests below are all about a declaring chain failing to seat what it
    /// declared, so they must not accidentally run against the explicit
    /// `none` posture, which is exempt by design.
    fn declaring() -> Vec<(&'static str, &'static str)> {
        vec![("chain_id", "wrfcoin-testnet")]
    }

    /// Metadata for the explicit "this chain has no oracles" posture.
    fn declares_none() -> Vec<(&'static str, &'static str)> {
        vec![
            (GENESIS_ORACLE_SET_KEY, GENESIS_ORACLE_NONE),
            (GENESIS_ORACLE_QUORUM_KEY, "0"),
        ]
    }

    #[test]
    fn a_correct_genesis_is_accepted_in_every_transaction_order() {
        let (set, k) = decided();
        let d = GenesisOracleDeclaration::new(set.clone(), k).unwrap();
        let encoded = d.encode_oracle_set();
        let mut r = regs(&set, k);
        for rotation in 0..r.len() {
            r.rotate_left(rotation.min(1));
            validate_block_oracle_registrations(meta(&encoded, "2"), r.clone())
                .expect("the decided N=3 K=2 genesis must be accepted in every order");
        }
    }

    /// THE HOLE #2488 CANNOT CLOSE. Every registration agrees with its siblings
    /// — and the set is not the one genesis declared. Accepted today; refused here.
    #[test]
    fn a_unanimous_but_wrong_set_is_refused() {
        let (declared, k) = decided();
        let d = GenesisOracleDeclaration::new(declared.clone(), k).unwrap();

        // An attacker's set: unanimous across all registrations, wholly wrong.
        let wrong = vec![key(9999_1), key(9999_2), key(9999_3)];
        let err = validate_block_oracle_registrations(
            meta(&d.encode_oracle_set(), "2"),
            regs(&wrong, 2),
        )
        .expect_err("a unanimous but wrong set must be refused against the declaration");
        assert!(matches!(
            err,
            GenesisOracleError::RegistrationDisagreesWithGenesis { .. }
        ));
    }

    /// The same shape one member off — the subtle version.
    #[test]
    fn a_unanimous_set_with_one_substituted_member_is_refused() {
        let (declared, k) = decided();
        let d = GenesisOracleDeclaration::new(declared.clone(), k).unwrap();
        let mut swapped = declared.clone();
        swapped[2] = key(4242);
        assert!(matches!(
            validate_block_oracle_registrations(
                meta(&d.encode_oracle_set(), "2"),
                regs(&swapped, 2)
            )
            .unwrap_err(),
            GenesisOracleError::RegistrationDisagreesWithGenesis { .. }
        ));
    }

    /// A unanimous but wrong QUORUM — right members, unmeetable threshold.
    #[test]
    fn a_unanimous_but_wrong_quorum_is_refused() {
        let (declared, k) = decided();
        let d = GenesisOracleDeclaration::new(declared.clone(), k).unwrap();
        assert!(matches!(
            validate_block_oracle_registrations(
                meta(&d.encode_oracle_set(), "2"),
                regs(&declared, 3)
            )
            .unwrap_err(),
            GenesisOracleError::RegistrationDisagreesWithGenesis { got_quorum: 3, .. }
        ));
    }

    /// #2523's opening case: block 1 declares a set and seats none of it.
    #[test]
    fn declaring_a_set_and_seating_none_is_refused() {
        let (declared, k) = decided();
        let d = GenesisOracleDeclaration::new(declared, k).unwrap();
        assert!(matches!(
            validate_block_one_seats_oracles(1, meta(&d.encode_oracle_set(), "2"), vec![])
                .unwrap_err(),
            GenesisOracleError::DeclaredButUnseated { .. }
        ));
    }

    /// Three registrations, all from the SAME operator — a count check passes,
    /// two declared oracles are still unseated behind the quorum.
    #[test]
    fn duplicate_identities_do_not_seat_the_set() {
        let (declared, k) = decided();
        let d = GenesisOracleDeclaration::new(declared.clone(), k).unwrap();
        let dupes: Vec<(Vec<String>, usize, String)> = (0..3)
            .map(|_| (declared.clone(), 2, declared[0].clone()))
            .collect();
        let err = validate_block_one_seats_oracles(1, meta(&d.encode_oracle_set(), "2"), dupes)
            .unwrap_err();
        match err {
            GenesisOracleError::IncompletelySeated { missing, .. } => {
                assert_eq!(missing.len(), 2, "two declared oracles never registered")
            },
            other => panic!("expected IncompletelySeated, got {other:?}"),
        }
    }

    /// A chain cut before #2523 declares nothing and is unaffected.
    #[test]
    fn a_genesis_without_a_declaration_is_unaffected() {
        let (set, k) = decided();
        validate_block_oracle_registrations(
            vec![("chain_id", "wrfcoin-testnet")],
            regs(&set, k),
        )
        .expect("no declaration means nothing to enforce");
    }

    #[test]
    fn half_a_declaration_is_refused() {
        let (set, _) = decided();
        let d = GenesisOracleDeclaration::new(set, 2).unwrap();
        let only_set = vec![(GENESIS_ORACLE_SET_KEY, d.encode_oracle_set())];
        let borrowed: Vec<(&str, &str)> =
            only_set.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert!(matches!(
            genesis_oracle_declaration(borrowed).unwrap_err(),
            GenesisOracleError::IncompleteDeclaration { .. }
        ));
    }

    #[test]
    fn the_declaration_rejects_what_the_chain_would_reject() {
        // Operator names, not ed25519 hex — the cut-day abort.
        assert!(matches!(
            GenesisOracleDeclaration::new(vec!["oracle-n5-pro".into()], 1).unwrap_err(),
            GenesisOracleError::InvalidOracleId { .. }
        ));
        // Unmeetable quorum, the #2523 shape.
        let (set, _) = decided();
        assert!(matches!(
            GenesisOracleDeclaration::new(set.clone(), 4).unwrap_err(),
            GenesisOracleError::QuorumOutOfRange { .. }
        ));
        assert!(matches!(
            GenesisOracleDeclaration::new(set, 0).unwrap_err(),
            GenesisOracleError::QuorumOutOfRange { .. }
        ));
    }


    // ── #2523 outcome gate: block 1 must install an oracle set ──────────

    /// Route 1 — nothing was ever submitted. Empty mempool, empty block.
    #[test]
    fn block_one_with_no_registrations_at_all_is_refused() {
        assert!(matches!(
            validate_block_one_seats_oracles(1, declaring(), vec![]).unwrap_err(),
            GenesisOracleError::BlockOneSeatsNoOracles
        ));
    }

    /// Route 2 — the one a mempool-based gate MISSES. Registrations were
    /// submitted and accepted (the operator holds an HTTP 201), then dropped at
    /// assembly by the submission-timing filter. The block that reaches this
    /// check carries an unrelated transaction and no registration.
    ///
    /// A gate phrased "block 1 must carry more than a coinbase" passes here and
    /// the chain still comes up dead. This one refuses.
    #[test]
    fn block_one_whose_registrations_were_dropped_at_assembly_is_refused() {
        // What survived assembly: no OracleRegistration among them.
        let survivors: Vec<(Vec<String>, usize, String)> = vec![];
        assert!(matches!(
            validate_block_one_seats_oracles(1, declaring(), survivors).unwrap_err(),
            GenesisOracleError::BlockOneSeatsNoOracles
        ));
    }

    /// A registration carrying an EMPTY set does not count as seating one —
    /// otherwise the gate is satisfied by a transaction that installs nothing.
    #[test]
    fn an_empty_set_registration_does_not_satisfy_the_gate() {
        let hollow = vec![(vec![String::new()], 2, key(2523_9))];
        assert!(matches!(
            validate_block_one_seats_oracles(1, declaring(), hollow).unwrap_err(),
            GenesisOracleError::BlockOneSeatsNoOracles
        ));
    }

    /// RED control: the correct block 1 passes, so the gate is specific rather
    /// than a blanket refusal of every height-1 block.
    #[test]
    fn a_correctly_seated_block_one_passes_the_gate() {
        let (set, k) = decided();
        validate_block_one_seats_oracles(1, declaring(), regs(&set, k))
            .expect("N=3 K=2 seated at block 1 must pass");
    }

    /// The gate would still refuse an unmeetable quorum it was about to install.
    #[test]
    fn block_one_installing_an_unmeetable_quorum_is_refused() {
        let (set, _) = decided();
        // One operator registers, but names a quorum of 3 — one seated, three needed.
        let lone = vec![(set.clone(), 3, set[0].clone())];
        assert!(matches!(
            validate_block_one_seats_oracles(1, declaring(), lone).unwrap_err(),
            GenesisOracleError::QuorumOutOfRange { quorum: 3, set_len: 1 }
        ));
    }

    /// Heights above 1 are untouched — a dropped registration mid-chain is
    /// recoverable and must not halt production.
    #[test]
    fn heights_above_one_are_unaffected() {
        for h in [2u64, 43_000, 1_000_000] {
            validate_block_one_seats_oracles(h, declaring(), vec![])
                .expect("only height 1 is gated");
        }
    }

    // ── #2523 cut-time configuration parsing ────────────────────────────

    /// The three config states are distinct, and the middle one is fail-closed.
    ///
    /// This is the producer-set lesson carried forward (#2501): there, "seeded
    /// but empty" and "never seeded" collapsed into one value, which made the
    /// fail-closed guarantee unenforceable because the dangerous state took the
    /// unenforced branch. Declared-and-wrong must not look like not-declared.
    ///
    /// Moved here from `proof_of_publishing` when `expected_genesis_oracle_set`
    /// was deleted. It now tests a plain value rather than process-global
    /// environment under a lock — which is itself the point of the redesign.
    #[test]
    fn the_three_cut_config_states_are_distinct() {
        // ABSENT -> nothing configured. Note this is NOT "no oracles": the cut
        // path turns it into a refusal, and only an explicit `none` is consent.
        for absent in [None, Some(""), Some("   ")] {
            assert_eq!(
                parse_cut_oracle_declaration(absent),
                Ok(None),
                "{absent:?} is absence, not a declaration"
            );
        }

        // DECLARED AND WRONG -> fail-closed, each for its own reason.
        let (set, _) = decided();
        for (raw, needle) in [
            (r#"{"oracle_set":[],"oracle_quorum":0}"#.to_string(), "empty"),
            (format!(r#"{{"oracle_set":["{}"],"oracle_quorum":0}}"#, set[0]), "unmeetable"),
            (format!(r#"{{"oracle_set":["{}"],"oracle_quorum":2}}"#, set[0]), "unmeetable"),
            ("not json at all".to_string(), "not valid JSON"),
            // An operator name instead of ceremony output — the cut-day abort.
            (r#"{"oracle_set":["oracle-n5-pro"],"oracle_quorum":1}"#.to_string(), "ed25519"),
        ] {
            let err = parse_cut_oracle_declaration(Some(&raw))
                .expect_err("a present-but-wrong declaration must fail closed");
            assert!(
                err.contains(needle),
                "config {raw:?} must fail for the right reason ({needle}), got: {err}"
            );
        }

        // DECLARED AND VALID -> the canonical encoding, order-insensitively.
        let shuffled = format!(
            r#"{{"oracle_set":["{}","{}","{}"],"oracle_quorum":2}}"#,
            set[2].to_uppercase(), set[0], set[1]
        );
        let (encoded, quorum) = parse_cut_oracle_declaration(Some(&shuffled))
            .expect("valid")
            .expect("some");
        let canonical = GenesisOracleDeclaration::new(set.clone(), 2)
            .unwrap()
            .encode_oracle_set();
        assert_eq!(
            encoded, canonical,
            "identities must be lowercased, sorted and deduplicated — the set is a SET, so a \
             config written in a different order must not change the bytes, and therefore must \
             not change state_root"
        );
        assert_eq!(quorum, "2");

        // THE EXPLICIT NO-ORACLES POSTURE -> a decision, not an absence.
        assert_eq!(
            parse_cut_oracle_declaration(Some("none")),
            Ok(Some(("none".to_string(), "0".to_string()))),
        );
        assert_eq!(
            parse_cut_oracle_declaration(Some("  NONE  ")),
            Ok(Some(("none".to_string(), "0".to_string()))),
            "the posture is trimmed and case-insensitive — it is written by hand"
        );
    }

    /// The stamped `none` posture round-trips through the metadata parser as a
    /// DECISION, not as absence. If these two ever disagree, a chain could
    /// declare no oracles and still be refused for declaring nothing.
    #[test]
    fn the_no_oracles_posture_round_trips_through_the_parser() {
        let (encoded, quorum) = parse_cut_oracle_declaration(Some("none"))
            .expect("valid")
            .expect("some");
        let meta = vec![
            (GENESIS_ORACLE_SET_KEY, encoded.as_str()),
            (GENESIS_ORACLE_QUORUM_KEY, quorum.as_str()),
        ];
        assert_eq!(
            genesis_oracle_declaration(meta),
            Ok(Some(GenesisOraclePosture::NoOracles))
        );
    }

    /// A `none` posture carrying a non-zero quorum is a half-stated decision —
    /// no oracles behind a quorum of two is the unmeetable shape this module
    /// exists to prevent, so it is refused rather than normalised.
    #[test]
    fn no_oracles_with_a_nonzero_quorum_is_refused() {
        let meta = vec![
            (GENESIS_ORACLE_SET_KEY, GENESIS_ORACLE_NONE),
            (GENESIS_ORACLE_QUORUM_KEY, "2"),
        ];
        assert!(matches!(
            genesis_oracle_declaration(meta).unwrap_err(),
            GenesisOracleError::QuorumOutOfRange { quorum: 2, set_len: 0 }
        ));
    }

    /// An explicitly oracle-less genesis that nonetheless seats a registration
    /// is refused. The posture is a statement about the chain, and a block that
    /// contradicts it is wrong however internally consistent it is.
    #[test]
    fn declaring_no_oracles_then_seating_some_is_refused() {
        let (set, k) = decided();
        assert!(matches!(
            validate_block_oracle_registrations(declares_none(), regs(&set, k)).unwrap_err(),
            GenesisOracleError::RegistrationDisagreesWithGenesis { .. }
        ));
        // ...and a coinbase-only block 1 on that same chain is fine.
        validate_block_one_seats_oracles(1, declares_none(), vec![])
            .expect("an explicitly oracle-less genesis seats nothing, by decision");
    }

    /// FRANK'S FINDING (#2540 review): `none` is not a free choice on the
    /// public testnet.
    ///
    /// The escape hatch was documented as uncatchable — a correctly-formed wrong
    /// decision only the ceremony checklist could mitigate. It is not. Block 1
    /// already carries `network_id`, so on the one network whose answer is known
    /// in advance the block itself can refuse, with no hardcoded N or K.
    ///
    /// This is his exact reported input.
    #[test]
    fn the_public_testnet_refuses_an_explicitly_oracle_less_genesis() {
        let mut m = declares_none();
        m.push((GENESIS_NETWORK_ID_KEY, "testnet"));
        assert!(
            matches!(
                validate_block_one_seats_oracles(1, m, vec![]).unwrap_err(),
                GenesisOracleError::PublicTestnetDeclaredNoOracles
            ),
            "network_id=testnet + oracle_set=none must be refused BEFORE the cut, \
             not caught by a checklist after it"
        );
    }

    /// RED CONTROL for the rule above — it must be specific, not a blanket ban
    /// on `none`.
    ///
    /// Dev and local chains genuinely have no ceremony, and `none` is how they
    /// say so. A guard that refused them would push every such chain back to
    /// declaring silence, which is the state #2523 exists to abolish.
    #[test]
    fn a_non_testnet_chain_may_still_declare_no_oracles() {
        // No network_id at all — the pre-#2545 shape.
        validate_block_one_seats_oracles(1, declares_none(), vec![])
            .expect("an explicitly oracle-less genesis seats nothing, by decision");

        for network in ["dev", "local", "mainnet", "", "  "] {
            let mut m = declares_none();
            m.push((GENESIS_NETWORK_ID_KEY, network));
            validate_block_one_seats_oracles(1, m, vec![]).unwrap_or_else(|e| {
                panic!("network_id={network:?} is not the public testnet and must keep `none`: {e}")
            });
        }
    }

    /// The predicate is a CLOSED comparison, deliberately.
    ///
    /// It gates a refusal that stops a genesis cut, so a neighbouring network
    /// must not inherit public-testnet policy by looking similar. Trimming and
    /// case folding are in; prefix and substring matching are out.
    #[test]
    fn the_testnet_predicate_matches_exactly_not_by_prefix() {
        for exact in ["testnet", "TESTNET", " testnet ", "TestNet"] {
            let mut m = declares_none();
            m.push((GENESIS_NETWORK_ID_KEY, exact));
            assert!(
                validate_block_one_seats_oracles(1, m, vec![]).is_err(),
                "{exact:?} IS the public testnet"
            );
        }
        for neighbour in ["testnet-2", "my-testnet", "testnets", "pre-testnet"] {
            let mut m = declares_none();
            m.push((GENESIS_NETWORK_ID_KEY, neighbour));
            assert!(
                validate_block_one_seats_oracles(1, m, vec![]).is_ok(),
                "{neighbour:?} is a DIFFERENT network and must not inherit \
                 public-testnet policy"
            );
        }
    }

    /// The refusal does not depend on the block being empty — a testnet cut that
    /// declares `none` is refused whatever else block 1 carries, so it cannot be
    /// worked around by including an unrelated registration.
    #[test]
    fn the_testnet_none_refusal_is_not_avoided_by_seating_something() {
        let (set, k) = decided();
        let mut m = declares_none();
        m.push((GENESIS_NETWORK_ID_KEY, "testnet"));
        assert!(matches!(
            validate_block_one_seats_oracles(1, m, regs(&set, k)).unwrap_err(),
            GenesisOracleError::PublicTestnetDeclaredNoOracles
        ));
    }

    /// The encoding is canonical: order in, same bytes out.
    #[test]
    fn the_encoding_is_order_independent_and_canonical() {
        let (set, k) = decided();
        let a = GenesisOracleDeclaration::new(set.clone(), k).unwrap();
        let mut shuffled = set;
        shuffled.reverse();
        let b = GenesisOracleDeclaration::new(shuffled, k).unwrap();
        assert_eq!(a.encode_oracle_set(), b.encode_oracle_set());
        assert_eq!(a, b, "declaration order must not change state_root");
    }
}
