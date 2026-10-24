# Early-warnings push channel: decision memo

Status: **Accepted (owner, 2026-10-24).** Transport P (no push yet; decide with data) was chosen by the owner
on 2026-10-24. The memo itself still picks nothing, and it authorizes no
implementation, provisioning, credential, account or release change.

Refs (private backlog, summarised here because the links are not public):

- `wrfcoin/core4-archive#3573`: the early-warnings epic. It sends advisory alerts (heavy rain and
  flash flood, extreme heat, severe wind) built from verified station data to people in data-sparse
  regions, **SMS-first** so that feature phones work. The alerts fill gaps and never replace or
  contradict official warnings.
- `wrfcoin/backend-archive#1500`: SMS delivery and opt-in subscriptions, built on the existing
  inbound SMS gateway. That gateway has no outbound path yet.
- `wrfcoin/mobile-archive#436`: in-app alert subscriptions and push delivery. Blocked on this
  decision.
- `wrfcoin/mobile-archive#271`: a decision record closed 2026-09-21 with **Option A**: server-initiated
  push is out of testnet scope, and alerts are delivered only while the app is open. Its options
  B (Firebase Cloud Messaging) and C (self-hosted UnifiedPush/ntfy) were left open for a later
  phase. This memo is that later phase's input.
- Public mobile repository: `docs/design/remote-push-notifications.md` (an FCM-shaped lifecycle,
  registry and payload-privacy design, written 2026-10-02). Most of its client and server rules
  apply to every option below.

## The question

> For the in-app half of early warnings, which transport carries a "something changed, open the
> app" signal to a phone whose app is closed: **FCM**, **UnifiedPush (self-hosted ntfy)**, a
> **hybrid** of the two, or **no push** (keep local-only alerts, optionally with periodic
> background polling)?

SMS is **not** an alternative to this choice. It is the epic's primary channel for feature phones
(`backend-archive#1500`) and stays in every option. This decision only concerns smartphones.

## What exists today (public repositories)

| Surface | State |
|---|---|
| Mobile `lib/main.dart` | No remote push registration at startup; the comment records that alerts are delivered while the app is open (#271 Option A). |
| Mobile `lib/features/weather/presentation/screens/alert_settings_screen.dart` | Shows the copy "Alerts are delivered while the app is open." |
| Mobile `lib/services/notifications/push_service.dart` | Local notifications via `flutter_local_notifications` (`pubspec.yaml`). |
| Mobile `lib/services/notifications/fcm_token_service.dart` | Token storage, rotation and retry logic written against an abstract `FcmPlatform`. No Firebase SDK in `pubspec.yaml` and no platform implementation, so it is inert. It targets `POST /notifications/push-token`. |
| Mobile `lib/services/notifications/notification_payload_router.dart` | Payload-to-route seam with tests. The mobile design note lists hardening needed before it accepts remote input. |
| Mobile `test/bootstrap/main_no_push_startup_test.dart` | Guards that no push registration runs at startup. |
| Mobile background work | `workmanager` is already a dependency (`lib/services/background/sensor_worker.dart`). |
| Backend | No push-token route, token registry or push sender exists. The alert engine (A1) and SMS subscription model (A2) are themselves post-GO work. |

Everything below therefore needs a backend registry, a sender and subscription sync, whatever the
transport. Those are shared costs, not a reason to prefer one option.

## Options

**F: Firebase Cloud Messaging.** Google's managed push service. On Android it is delivered through
Google Play services; on iOS through Apple Push Notification service (APNs) relayed by Firebase.

**U: UnifiedPush with a self-hosted ntfy server.** An open push protocol. The app registers with a
*distributor* app the user installs (for example ntfy), and the distributor holds one connection to
a push server. The backend POSTs to a per-registration endpoint URL on that server. Android only.

**H: Hybrid.** The app supports UnifiedPush when a distributor is installed. Otherwise it falls back
to FCM, through UnifiedPush's embedded FCM distributor or a direct FCM path. On iOS it uses APNs via
FCM.

**P: No push.** Keep #271 Option A (local alerts while the app is open). Optionally add a periodic
Android background fetch of active alerts through the existing `workmanager`. No third-party push
service is involved.

## Comparison

Points marked **(verify)** are general platform behaviour that must be confirmed against current
vendor documentation before anyone implements. They were not measured for this memo.

| Criterion | F: FCM | U: UnifiedPush / self-hosted ntfy | H: Hybrid | P: No push / poll |
|---|---|---|---|---|
| **Google dependency** | Yes: Firebase project, Play services on the device and Google as a processor. | None if the distributor and server are self-hosted. | Only for users without a distributor, and for iOS. | None. |
| **Payload privacy** | Transport-encrypted, but readable by the provider unless the app encrypts it itself. The mobile design note already requires a generic, location-free envelope. | With Web Push encryption (RFC 8291) the payload is end-to-end encrypted to the device **(verify for the chosen library)**. The server operator still sees endpoint and timing. | Per path, as F or U. | No payload leaves the project's own API. |
| **Metadata exposure** | Google learns token, app identity, delivery timing and volume per device. | The project's push server learns endpoint and timing. A public ntfy.sh instance would expose these to that operator instead. | As F for fallback users. | The project's API learns poll timing (it already sees app traffic). |
| **Reliability / latency** | Managed, typically seconds. High-priority messages can wake a dozing device; Android may deprioritise apps whose high-priority pushes show no notification **(verify)**. | Depends on our server's uptime and the distributor's persistent connection. Typically seconds while connected; delayed when the connection is killed by OEM battery savers **(verify per vendor)**. | Best of both on Android, but two delivery paths to test. | Latency ≥ the poll period (Android `WorkManager` periodic minimum is 15 min) and not guaranteed under Doze or OEM kill policies. Nothing arrives while the app is force-stopped. |
| **Battery** | Shares the single system Play-services connection. Lowest marginal cost. | One extra persistent connection held by the distributor app, shared by every UnifiedPush app on the device. Usually a visible foreground-service notification **(verify)**. | As U for distributor users, F otherwise. | One wake-up per poll period. Modest, but periodic network use with no event. |
| **De-Googled Android** (no Play services; or microG) | Not delivered without Play services; partial via microG **(verify)**. | Works: this is UnifiedPush's purpose. | Works. | Works. |
| **iOS** | Works via APNs (requires Apple push entitlement and an APNs key). | **No UnifiedPush on iOS.** iOS would stay local-only, or use APNs separately. | APNs via FCM. | Local-only. iOS background fetch is opportunistic **(verify)**. |
| **Cost (zero-budget rule)** | FCM messaging is free of charge. iOS requires an Apple developer membership, a recurring fee that applies only if an iOS build is distributed, which is a separate decision. | Server software is free. It runs on an existing project host, so the only cost is ops time. A paid ntfy.sh tier is not needed if self-hosted. | Union of F and U. | Free. |
| **Operational burden** | Firebase project and console, a server sender credential (service account or workload identity) and its rotation, client config files as CI secrets, APNs key custody. No server to run. | Run, patch, monitor and TLS-terminate a public push server. Configure access control so it is not an open relay. Back up its state. No third-party secrets. | Both. | None beyond the existing API. |
| **Abuse / spam controls** | Only holders of the sender credential can send. Compromise of that credential means pushes to all users, so the credential needs tight custody. | Endpoints are capability URLs. A leaked endpoint lets anyone push to that device until it is rotated. A self-hosted server must default to deny-all publish with per-sender auth and rate limits. Never use guessable public topics. | Both sets. | Only our authenticated API serves content, so there is nothing to spoof. |
| **User friction** | None on Google Android; denied silently on de-Googled devices. | The user must install and choose a distributor app, which is a real onboarding step for non-technical users in the target regions **(product risk)**. | Distributor optional. | None, but users must open the app, or wait for the poll, to be warned. |
| **Supply chain / app size** | Adds Firebase SDKs (and Play-services coupling) to the app. | Adds a small UnifiedPush client library. | Both. | Nothing new. |
| **Reversibility** | Easy to remove (the SDK is behind a flag). Firebase project ownership persists. | Easy to remove. The server can be retired. | Easy. | Already the state today. |

### Constraints common to every push option

These carry over from the mobile design note and the epic, and apply to F, U and H alike:

- **Payload carries no location at any precision** and no wallet, balance or reading data. It is a
  generic "open the app" envelope. Content is fetched over the app's authenticated, coarse-location
  API on tap.
- **Advisory, not official.** Official warnings must be visually distinct, and copy must never
  imply a guaranteed or life-safety delivery. Delivery after force-stop or while offline is not
  claimed.
- **Opt-in only.** No registration before explicit consent, OS permission and an authenticated
  wallet. Revocation applies on sign-out and wallet change.
- **One subscription model** shared with SMS (`backend-archive#1500`): coarse cell, hazards,
  language and quiet hours.
- **No secrets in any repository.** Client configuration and server credentials go through the
  release process.
- The backend registry, sender, retry/outbox and "never contradict official" policy are needed
  whatever the transport.

## What the mobile app would need, per option

| Work item | F | U | H | P |
|---|---|---|---|---|
| Overturn #271 Option A on record and update the "while the app is open" copy | yes | yes | yes | no (copy stays; a poll option adds "checked every N minutes") |
| `firebase_core` / `firebase_messaging` + a real `FcmPlatform`; auto-init and analytics disabled | yes | no | yes | no |
| `google-services.json` / `GoogleService-Info.plist` as CI secrets; APNs key | yes | no | yes | no |
| UnifiedPush client library; distributor discovery and selection UI; endpoint (not token) registration | no | yes | yes | no |
| Generalise `FcmTokenService` into a transport-neutral registration service (`kind: fcm_token` / `unifiedpush_endpoint`) | rename optional | yes | yes | no |
| Harden `notification_payload_router.dart` for remote input (per the mobile design note) | yes | yes | yes | no |
| `workmanager` periodic alert fetch + local notification | no | no | no | optional |
| Backend: registration route, encrypted registry, sender, revocation, subscription sync | FCM HTTP v1 sender | Web Push sender (VAPID keys) to UnifiedPush endpoints | both senders | none (alerts API only) |
| Infra: push server | none | ntfy (or equivalent) with TLS, auth, monitoring | ntfy | none |

## Decision table

| If the owner prioritises… | …the option that best fits | …and what it costs |
|---|---|---|
| Lowest operations work and iOS coverage | **F** | Google dependency and metadata; Firebase secrets; de-Googled users get nothing. |
| No Google dependency and self-hosting (Android only) | **U** | A push server to run; distributor-install friction; iOS stays local-only. |
| Reach on every Android device, plus iOS | **H** | The most implementation and test surface; still a Google path for most users. |
| Zero new dependencies now, decide later with data | **P** | No timely warning while the app is closed (poll latency ≥ 15 min, not guaranteed). SMS remains the timely channel. |

## Questions for the owner (please answer on `wrfcoin/mobile-archive#436`, or by reopening `#271`)

1. **Transport:** F, U, H or P?
2. **Platforms:** Android only for the first slice, or Android and iOS? (If iOS is included, F or H
   is implied for iOS, and an Apple developer membership is required.)
3. **If U or H:** is running a public push server on project infrastructure acceptable, given the
   ops time and the need for a deny-by-default publish policy? Or should a third-party ntfy
   instance be considered, given its metadata exposure to that operator?
4. **If F or H:** who holds the Firebase project and the server sender credential, and who rotates
   it?
5. **Order:** should in-app push wait until the SMS path (A2) and the alert engine (A1) have
   measured a false-alarm rate, as the epic's risk section suggests?

Until the owner answers, #271 Option A stands: alerts are delivered while the app is open, and no
remote push code ships.

## Decision (accepted 2026-10-24)

Owner ruling, 2026-10-24: "Merge as accepted, recommended answers." Each open question gets the
answer this document itself recommends, quoted with its line. Where the document makes no
recommendation, the question was left open for the owner and is answered below as `Decision (owner, 2026-10-24)`: none was invented.

The memo states no recommendation for any of its questions (line 3 originally: "This memo picks nothing"), so
every question was left for the owner and is answered below. Its own rule for the interim stays in force
(line 143: "Until the owner answers, #271 Option A stands"): alerts are delivered while the app is open,
and no remote push code ships. The owner's answers keep it that way: transport P means no push yet.

1. *Transport: F, U, H or P?* **Decision (owner, 2026-10-24): P — no push yet; decide with data. Alerts are delivered while the app is open, and SMS is the timely channel. No remote push code ships.** The comparison and decision tables weigh the
   options without choosing one.
2. *Platforms: Android only for the first slice, or Android and iOS?* **Decision (owner, 2026-10-24): Android only first, when push comes.**
3. *If U or H: run a public push server on project infrastructure, or consider a third-party ntfy
   instance?* **Decision (owner, 2026-10-24): Self-host on project infrastructure, if U or H is chosen later.** Depends on question 1.
4. *If F or H: who holds the Firebase project and sender credential, and who rotates it?*
   **Decision (owner, 2026-10-24): N/A for now (transport P); no Firebase credential is created. Revisit if F or H is chosen later.** Depends on question 1.
5. *Order: should in-app push wait until the SMS path (A2) and alert engine (A1) have measured a
   false-alarm rate?* **Decision (owner, 2026-10-24): Yes: measure the SMS path (A2) and alert-engine (A1) false-alarm rate first, before any in-app push.** Relevant text (not a recommendation): line 141: "as
   the epic's risk section suggests". The memo relays the epic's suggestion inside the question; it
   does not recommend it itself.
