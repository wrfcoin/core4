# Early warnings: pilot and liability research

Status: DRAFT — owner and local experts have not ratified a pilot or public delivery.
Research checked 2026-10-28. Refs wrfcoin/core4-archive#3574 and
wrfcoin/core4-archive#3573. No partner was contacted and no commitment was made.
This is not legal advice; it needs a lawyer in each proposed jurisdiction.

## Candidate regions and authoritative sources

These are research candidates, not claims of measured WRFCoin station scarcity.
No public station-density export was available to this lane. Before selecting a
cell, obtain an anonymised, dated registry snapshot and count independently verified
active stations per coarse cell; distinguish missing coverage from a failed export.
Rank low-count cells with local hazard and response expertise. Do not publish owner
names, exact station coordinates or unsubstantiated scarcity maps.

| Candidate | Official warning source | Research rationale and missing evidence |
|---|---|---|
| Northern/eastern Kenya | [Kenya Meteorological Department warnings](https://meteo.go.ke/weather-warnings/) | First research candidate: the source publishes heavy-rain and strong-wind advisories with issue times. Confirm local station gaps and community needs before choosing a county/cell. |
| Southern Malawi | [DCCMS weather services](https://www.metmalawi.gov.mw/Services/weather/) | Alternate candidate for rain/wind studies. DCCMS identifies weather warnings among its public services. Local station coverage, warning archive and outcome labels still needed. |
| Southeastern Madagascar | [Météo Madagascar vigilance](https://www.meteomadagascar.mg/vigilance/) and [cyclone information](https://www.meteomadagascar.mg/cyclone/) | Alternate candidate; vigilance covers rain, wind, waves and floods, with a separate cyclone system. French template tests are not Malagasy-language validation or permission to relay cyclone alerts. |

Recommendation: begin a Kenya **offline data feasibility study**, conditional on
owner selection and access to matched histories. Do not choose a public pilot cell
from this desk research alone. The research thresholds borrowed from HKO/NWS in
backend are not Kenyan, Malawian or Malagasy warning criteria.

## Official relay, provenance and terms

[WMO's CAP overview](https://wmo.int/site/wmo-common-alerting-protocol) describes
structured authoritative alerts carrying hazard, severity, urgency, certainty and
area. CAP is a preferred adapter format, not proof that a particular country's
public website exposes a stable API or grants redistribution rights.

| Source | Before ingest or relay | Current disposition |
|---|---|---|
| KMD | Confirm authoritative CAP/feed endpoint, archive completeness, polygon/geocode mapping, cancellations and updates; obtain applicable licence/attribution and redistribution terms. | Public warning page located; machine feed and reuse permission unverified. |
| DCCMS | Ask the official service to identify current alert/archive feeds, geographic coverage and language; review terms and fallback procedure. | Public service description located; feed and reuse permission unverified. |
| Météo Madagascar | Confirm vigilance plus cyclone feed coverage, issue/expiry semantics and official Malagasy/French text; review translation and redistribution terms. | Public pages located; feed and reuse permission unverified. |

Never scrape a failed or incomplete response into an empty 'no warning' set. Keep
source ID, URL, timestamps, coverage and provenance; fail closed when completeness
cannot be established. Map all intersected cells conservatively. An overlapping
national warning suppresses the independent event and remains the authoritative
pointer. Do not paraphrase an official instruction into contrary advice. Backend
`docs/alerts/official-precedence-policy.md` maps implemented rules to tests.

## Candidate partner brief: Climate Centre and national societies

The [Red Cross Red Crescent Climate Centre](https://www.climatecentre.org/priority_areas/fbf-ibf/)
describes anticipatory action and early-warning/early-action support. A possible
role is methods review with the relevant national society, not endorsement of a
new warning issuer. Proposed discussion packet: anonymised detector/outcome replay,
missed-event analysis, local action options, language-comprehension tests and the
official precedence policy. Ask whether useful actions can follow a warning and
who owns correction/escalation. Likely concerns to test with them (our inference,
not an attributed position): false alarms, volunteer burden, exclusion of people
without phones, confusion with official branding and unvalidated thresholds.
No agreement, named contact or partnership claim is implied.

## Candidate partner brief: WMO Early Warnings for All

[WMO's action-plan overview](https://wmo.int/resources/wmo-bulletin/wmo-bulletin-vol-72-1-2023/overview-of-early-warnings-all-executive-action-plan-2023-2027)
frames warning systems across risk knowledge, observation/forecasting,
dissemination and preparedness. A proposed contribution is evidence about gaps in
station coverage and audited relay fidelity, aligned with national services.
Bring the observation provenance contract and coverage/outcome study, not a claim
that an SMS interface constitutes an end-to-end warning system. Questions for an
authorised future discussion: which national coordination process applies, how to
verify alert authority and which accessibility measures are required? An anticipated
concern is parallel unofficial warning authority; this is our planning inference.

## Candidate partner brief: national services, NGOs and telecoms

KMD, DCCMS and Météo Madagascar would each need a jurisdiction-specific brief:
requested read-only archive window, exact fields/units, source attribution, local
threshold review, correction/cancellation handling and a proposal for shadow-only
comparison. Ask each to identify limitations rather than assuming a global weather
threshold transfers. No live feed agreement is currently in hand.

Local disaster-management/community organisations could review comprehension,
trusted languages, accessibility and feasible actions; participation must not be
inferred from geographic presence. A telecom or aggregator could supply an
opt-in/STOP sandbox, delivery receipts and written per-segment pricing. Anticipated
questions (our inference): who authenticates consent, bears cost, handles pumping
abuse, and stops messages during an incident? Zero budget means no paid messages;
credits or donated service require an actual agreement. Cell broadcast requires
coordination with competent authorities, not a substitute for that process.

## Disclaimer and liability review packet

Proposed full-form wording for counsel and community testing:

> WRFCoin research advisories may be incomplete or delayed. They do not replace
> official weather warnings or emergency instructions. Follow your national weather
> service and local authorities. The absence of an advisory does not mean safety.

This wording is a proposal, not a legal shield. The short SMS research templates
cannot establish informed consent by themselves. Keep official attribution intact;
never claim guaranteed safety, guaranteed warning delivery or official affiliation.

Kenya counsel should assess the [ODPC consent guidance](https://www.odpc.go.ke/wp-content/uploads/2024/02/ODPC-Guidance-Notes-on-Consent.pdf)
and [regulator guidance index](https://www.odpc.go.ke/guidelines-2/) against the actual
phone-hash, location, consent and retention design. These sources identify a review
starting point; this document makes no determination that the design complies.
For Malawi and Madagascar, qualified local counsel must identify the applicable
privacy, telecom, emergency-warning and cross-border processing rules; no local
legal review was performed. Phone hashes remain potentially linkable identifiers.

Questions requiring disposition: who is the responsible operator/controller;
what consent evidence and deletion process are adequate; what warning-relay licence
is required; who handles missed/incorrect warnings and corrections; what accessible
terms are understood; whether professional/public liability or cyber insurance is
needed and available; and which responsibilities must appear in a partner agreement.
No disclaimer or insurance purchase authorises a deployment.

## Go/no-go recommendation

**GO only for offline research and synthetic tests. NO-GO for public alerts.**
Before reconsideration, require a selected pilot and verified sparse-cell evidence;
a complete official feed with reuse permission; historical per-hazard precision,
recall and false-alarm/missed-event results with owner-approved acceptance bounds;
local expert review of thresholds and outcomes; counsel-reviewed consent and
liability posture; authenticated opt-in and immediate STOP; funded/approved
transport; correction and outage drills; and explicit owner release approval.

Settled transport P remains in force: no push now; in-app research while open;
SMS delivery remains absent. If push is revisited, Android first/self-hosted is
already the owner direction. There is no Firebase work or consensus change here.
