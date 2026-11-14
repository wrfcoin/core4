//! `station_owner_key_audit <path/to/COPY/blocks.redb> [recent_blocks=2000]`:
//! counts only. Logic and doc: `wrfcoin_native_chain::station_owner_audit`.
//! Run ONLY on a copy of a node's block DB, never on a live node volume.

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(db) = args.next() else {
        eprintln!("usage: station_owner_key_audit <path/to/COPY/blocks.redb> [recent_blocks=2000]");
        std::process::exit(2);
    };
    let recent_blocks = args.next().map_or(Ok(2_000), |r| r.parse::<u64>()).unwrap_or_else(|_| {
        eprintln!("recent_blocks must be an integer");
        std::process::exit(2);
    });
    let report = wrfcoin_native_chain::station_owner_audit::run_audit(std::path::Path::new(&db), recent_blocks)
        .unwrap_or_else(|e| {
            eprintln!("AUDIT_FAILED: {e}");
            std::process::exit(1);
        });
    let (tip, counts) = (report.tip, &report.counts);
    let check = if report.commitment_compared {
        "tip state-root commitment verified"
    } else {
        "protocol V1 tip: no state-root commitment to compare"
    };
    println!("station owner key audit at tip {tip} (recent = last {recent_blocks} blocks; counts only; replayed under the chain's own schedule; {check})");
    println!("  stations registered:                         {}", counts.stations);
    println!("  stations active:                             {}", counts.active_stations);
    println!("  distinct owners:                             {}", counts.owners);
    println!("  owners seen with an ed25519 key (can sign):  {}", counts.owners_with_ed25519_key);
    println!("  owners passkey-only or unknown (cannot):     {}", counts.owners_passkey_or_unknown);
    println!("  active stations of those owners:            {}", counts.active_stations_of_passkey_or_unknown_owners);
    println!("  stations with recent commits:                {}", counts.stations_with_recent_commits);
    println!("    signed by a non-owner key (need approval): {}", counts.stations_with_recent_non_owner_commits);
    println!("    of which the owner cannot sign today:      {}", counts.of_which_owner_cannot_sign);
    println!("AUDIT_OK");
}
