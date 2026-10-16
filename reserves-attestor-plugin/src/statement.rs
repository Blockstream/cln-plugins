use chrono::{DateTime, Utc};

pub const BEGIN_MESSAGE: &str = "-----BEGIN CLN SIGNED MESSAGE-----";
pub const BEGIN_SIGNATURE: &str = "-----BEGIN CLN SIGNATURE-----";
pub const END_SIGNATURE: &str = "-----END CLN SIGNATURE-----";

/// Bumped whenever the statement layout changes. Lets a parser pick the
/// right rules for the statement it's looking at instead of guessing from
/// shape.
pub const STATEMENT_VERSION: u32 = 1;

/// Everything needed to render a balance statement, already reduced to plain
/// msat amounts - Lightning's native unit, so nothing has to be rounded to
/// fit. Caveated amounts (unconfirmed, reserved, immature, pending channel)
/// are tracked separately from the totalled amounts so they can never
/// accidentally leak into `total_msat`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BalanceStatementInputs {
    pub node_id: String,
    pub network: String,
    /// RFC 3339, e.g. `2026-09-22T10:00:00Z` datetime.
    pub statement_time: DateTime<Utc>,
    pub block_height: u32,

    /// Confirmed, unreserved wallet UTXOs - the only on-chain funds counted.
    pub onchain_confirmed_msat: i64,
    pub onchain_unconfirmed_msat: i64,
    pub onchain_reserved_msat: i64,
    pub onchain_immature_msat: i64,

    /// Our side of channels in CHANNELD_NORMAL - the only Lightning funds counted.
    pub lightning_open_msat: i64,
    pub lightning_pending_msat: i64,
}

impl BalanceStatementInputs {
    pub fn total_msat(&self) -> i64 {
        self.onchain_confirmed_msat + self.lightning_open_msat
    }
}

/// Builds the exact block of text that gets signed - the content between the
/// BEGIN/END CLN SIGNED MESSAGE armor, not including the armor itself.
pub fn build_message_block(service_name: &str, inputs: &BalanceStatementInputs) -> String {
    let lines = vec![
        format!("{service_name} node balance statement"),
        String::new(),
        format!("Statement version:    {STATEMENT_VERSION}"),
        format!("Node ID:              {}", inputs.node_id),
        format!("Network:              {}", inputs.network),
        format!(
            "Statement time:       {}",
            inputs.statement_time.format("%Y-%m-%dT%H:%M:%SZ")
        ),
        format!("Block height:         {}", inputs.block_height),
        String::new(),
        format!(
            "On-chain balance:     {} msats  (confirmed, spendable by this node)",
            inputs.onchain_confirmed_msat
        ),
        format!(
            "Lightning balance:    {} msats  (this node's side of open channels)",
            inputs.lightning_open_msat
        ),
        format!("Total under control:  {} msats", inputs.total_msat()),
        String::new(),
        // Reserves the slot for a CSV hash later, so adding one is a value
        // change on this line, not a format change.
        "Accounting data:      none".to_string(),
        String::new(),
        "Excluded from totals:".to_string(),
        format!(
            "  - {:<36}{} msats",
            "Unconfirmed on-chain:", inputs.onchain_unconfirmed_msat
        ),
        format!(
            "  - {:<36}{} msats",
            "Reserved on-chain UTXOs:", inputs.onchain_reserved_msat
        ),
        format!(
            "  - {:<36}{} msats",
            "Immature on-chain, e.g. coinbase:", inputs.onchain_immature_msat
        ),
        format!(
            "  - {:<36}{} msats",
            "Pending channels, opening/closing:", inputs.lightning_pending_msat
        ),
    ];

    lines.join("\n")
}

/// Wraps a signed message block with the armor the verifier is told to expect.
/// Only `message_block` was passed to `signmessage` - the armor is added
/// after the fact and is never itself part of the signed content.
pub fn build_signed_statement(message_block: &str, zbase_signature: &str) -> String {
    format!(
        "{BEGIN_MESSAGE}\n{message_block}\n{BEGIN_SIGNATURE}\n{zbase_signature}\n{END_SIGNATURE}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_inputs() -> BalanceStatementInputs {
        BalanceStatementInputs {
            node_id: "02aabb".to_string(),
            network: "bitcoin".to_string(),
            statement_time: DateTime::parse_from_rfc3339("2026-09-17T12:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            block_height: 900_000,
            onchain_confirmed_msat: 100_000_000,
            onchain_unconfirmed_msat: 0,
            onchain_reserved_msat: 0,
            onchain_immature_msat: 0,
            lightning_open_msat: 50_000_000,
            lightning_pending_msat: 0,
        }
    }

    #[test]
    fn service_name_selects_heading() {
        let inputs = base_inputs();
        assert!(
            build_message_block("Example Service", &inputs)
                .starts_with("Example Service node balance statement")
        );
    }

    #[test]
    fn total_sums_only_confirmed_and_open() {
        let mut inputs = base_inputs();
        inputs.onchain_unconfirmed_msat = 1_000;
        inputs.onchain_reserved_msat = 2_000;
        inputs.onchain_immature_msat = 3_000;
        inputs.lightning_pending_msat = 4_000;

        assert_eq!(inputs.total_msat(), 150_000_000);
    }

    #[test]
    fn message_block_contains_required_fields_and_zeroed_exclusions_when_clean() {
        let inputs = base_inputs();
        let msg = build_message_block("Core Lightning", &inputs);

        assert!(msg.starts_with("Core Lightning node balance statement"));
        assert!(msg.contains("Statement version:    1"));
        assert!(msg.contains("Node ID:              02aabb"));
        assert!(msg.contains("Network:              bitcoin"));
        assert!(msg.contains("Statement time:       2026-09-17T12:00:00Z"));
        assert!(msg.contains("Block height:         900000"));
        assert!(msg.contains("On-chain balance:     100000000 msats"));
        assert!(msg.contains("Lightning balance:    50000000 msats"));
        assert!(msg.contains("Total under control:  150000000 msats"));
        assert!(msg.contains("Accounting data:      none"));
        // The exclusions section is always present, with zeros - "missing"
        // and "zero" must never be confused.
        assert!(msg.contains("Excluded from totals:"));
        assert!(msg.contains("Unconfirmed on-chain:"));
        assert!(msg.contains("Reserved on-chain UTXOs:"));
        assert!(msg.contains("Immature on-chain, e.g. coinbase:"));
        assert!(msg.contains("Pending channels, opening/closing:"));
        assert!(!msg.contains(BEGIN_MESSAGE));
    }

    #[test]
    fn message_block_reports_caveats_on_separate_lines_excluded_from_total() {
        let mut inputs = base_inputs();
        inputs.onchain_unconfirmed_msat = 1_000;
        inputs.onchain_reserved_msat = 2_000;
        inputs.onchain_immature_msat = 3_000;
        inputs.lightning_pending_msat = 4_000;

        let msg = build_message_block("Core Lightning", &inputs);

        assert!(msg.contains("Excluded from totals:"));
        assert!(msg.contains("Unconfirmed on-chain:"));
        assert!(msg.contains("1000 msats"));
        assert!(msg.contains("Reserved on-chain UTXOs:"));
        assert!(msg.contains("2000 msats"));
        assert!(msg.contains("Immature on-chain, e.g. coinbase:"));
        assert!(msg.contains("3000 msats"));
        assert!(msg.contains("Pending channels, opening/closing:"));
        assert!(msg.contains("4000 msats"));
        // Caveated amounts must never appear in the totalled lines.
        assert!(msg.contains("On-chain balance:     100000000 msats"));
        assert!(msg.contains("Lightning balance:    50000000 msats"));
        assert!(msg.contains("Total under control:  150000000 msats"));
    }

    #[test]
    fn signed_statement_wraps_exact_message_with_armor() {
        let inputs = base_inputs();
        let msg = build_message_block("Core Lightning", &inputs);
        let signed = build_signed_statement(&msg, "ZBASE32SIGNATURE");

        let expected = format!(
            "{BEGIN_MESSAGE}\n{msg}\n{BEGIN_SIGNATURE}\nZBASE32SIGNATURE\n{END_SIGNATURE}\n"
        );
        assert_eq!(signed, expected);

        // The signature block must wrap the message verbatim - the verifier strips
        // only the outer armor before calling checkmessage.
        let inner = signed
            .strip_prefix(BEGIN_MESSAGE)
            .unwrap()
            .trim_start_matches('\n');
        assert!(inner.starts_with(&msg));
    }
}
