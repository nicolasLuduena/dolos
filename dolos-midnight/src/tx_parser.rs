use dolos_core::BlockSlot;

use crate::block_source::RawSubstrateBlock;
use crate::chain::MidnightError;

// ---------------------------------------------------------------------------
// Parsed types — output of ExtrinsicParser
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum TxType {
    /// A regular Midnight transaction (send_mn_transaction)
    Regular,
    /// A system/inherent extrinsic (timestamp, etc.)
    System,
}

#[derive(Debug, Clone)]
pub struct ShieldedOutput {
    pub output_index: u32,
    /// Raw ciphertext blobs for trial decryption with viewing keys.
    pub ciphertexts: Vec<Vec<u8>>,
    /// The raw serialized coin data.
    pub raw_coin: Vec<u8>,
    /// Ledger version tag (7 or 8).
    pub ledger_version: u16,
}

#[derive(Debug, Clone)]
pub struct ShieldedInput {
    /// Nullifier identifying which UTxO is being spent.
    pub nullifier: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct UnshieldedOutput {
    pub owner: [u8; 32],
    pub token_type: [u8; 32],
    pub value: u128,
    pub output_index: u32,
}

#[derive(Debug, Clone)]
pub struct UnshieldedInput {
    pub owner: [u8; 32],
    pub token_type: [u8; 32],
    pub value: u128,
}

#[derive(Debug, Clone)]
pub struct ParsedTransaction {
    pub tx_hash: [u8; 32],
    pub tx_type: TxType,
    pub shielded_outputs: Vec<ShieldedOutput>,
    pub shielded_inputs: Vec<ShieldedInput>,
    pub unshielded_outputs: Vec<UnshieldedOutput>,
    pub unshielded_inputs: Vec<UnshieldedInput>,
    /// Original serialized tx bytes for storage.
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ParsedBlock {
    pub slot: BlockSlot,
    pub hash: [u8; 32],
    pub timestamp: Option<u64>,
    pub transactions: Vec<ParsedTransaction>,
}

// ---------------------------------------------------------------------------
// ExtrinsicParser trait
// ---------------------------------------------------------------------------

/// Parses pre-extracted midnight transaction bytes into structured data.
///
/// The `SubxtBlockSource` has already decoded the SCALE extrinsic envelope
/// and extracted the raw midnight tx payloads. The parser uses
/// `midnight_serialize::tagged_deserialize` to parse them into ledger types
/// and extract shielded outputs, inputs, and hashes.
pub trait ExtrinsicParser: Send + Sync + 'static {
    fn parse_block(&self, block: &RawSubstrateBlock) -> Result<ParsedBlock, MidnightError>;
}

// ---------------------------------------------------------------------------
// MockParser — for testing
// ---------------------------------------------------------------------------

pub struct MockParser;

impl ExtrinsicParser for MockParser {
    fn parse_block(&self, block: &RawSubstrateBlock) -> Result<ParsedBlock, MidnightError> {
        Ok(ParsedBlock {
            slot: block.number,
            hash: block.hash,
            timestamp: block.timestamp,
            transactions: Vec::new(),
        })
    }
}

// ---------------------------------------------------------------------------
// SubxtParser — real implementation (behind subxt-sync feature)
//
// Uses midnight-ledger types to parse the already-extracted midnight tx bytes.
// The extrinsic SCALE decoding was done in SubxtBlockSource during sync;
// here we only need midnight_serialize::tagged_deserialize.
// ---------------------------------------------------------------------------

#[cfg(feature = "subxt-sync")]
mod real_parser {
    use midnight_serialize::Serializable;
    use midnight_storage_core::db::InMemoryDB;
    use tracing::{debug, warn};

    use crate::block_source::TxKind;

    use super::*;

    /// Erased transaction type — no signatures/proofs needed for read-only parsing.
    type ErasedTransaction = midnight_ledger::structure::Transaction<
        (),
        (),
        midnight_transient_crypto::commitment::Pedersen,
        InMemoryDB,
    >;

    pub struct SubxtParser;

    impl ExtrinsicParser for SubxtParser {
        fn parse_block(&self, block: &RawSubstrateBlock) -> Result<ParsedBlock, MidnightError> {
            let mut transactions = Vec::new();

            for (idx, raw_tx) in block.midnight_txs.iter().enumerate() {
                match raw_tx.kind {
                    TxKind::Regular => match parse_regular_tx(&raw_tx.raw, idx) {
                        Ok(tx) => transactions.push(tx),
                        Err(e) => {
                            warn!(
                                block = block.number,
                                tx_idx = idx,
                                error = %e,
                                "failed to parse regular tx, skipping"
                            );
                        }
                    },
                    TxKind::System => match parse_system_tx(&raw_tx.raw, idx) {
                        Ok(tx) => transactions.push(tx),
                        Err(e) => {
                            warn!(
                                block = block.number,
                                tx_idx = idx,
                                error = %e,
                                "failed to parse system tx, skipping"
                            );
                        }
                    },
                }
            }

            debug!(
                block = block.number,
                tx_count = transactions.len(),
                "parsed block"
            );

            Ok(ParsedBlock {
                slot: block.number,
                hash: block.hash,
                timestamp: block.timestamp,
                transactions,
            })
        }
    }

    /// Parse a regular Midnight transaction using tagged_deserialize.
    fn parse_regular_tx(
        raw_tx: &[u8],
        tx_idx: usize,
    ) -> Result<ParsedTransaction, MidnightError> {
        let tx: ErasedTransaction =
            midnight_serialize::tagged_deserialize(&mut &raw_tx[..]).map_err(|e| {
                MidnightError::Decode(format!("tx {tx_idx}: tagged_deserialize failed: {e}"))
            })?;

        let tx_hash = tx.transaction_hash().0 .0;

        let mut shielded_outputs = Vec::new();
        let mut shielded_inputs = Vec::new();
        let mut output_idx: u32 = 0;

        match &tx {
            midnight_ledger::structure::Transaction::Standard(std_tx) => {
                // Extract from guaranteed_coins offer
                if let Some(offer) = &std_tx.guaranteed_coins {
                    extract_offer_outputs(offer, &mut shielded_outputs, &mut output_idx);
                    extract_offer_inputs(offer, &mut shielded_inputs);
                }

                // Extract from fallible_coins offers
                for offer in std_tx.fallible_coins.values() {
                    extract_offer_outputs(&offer, &mut shielded_outputs, &mut output_idx);
                    extract_offer_inputs(&offer, &mut shielded_inputs);
                }
            }
            midnight_ledger::structure::Transaction::ClaimRewards(_) => {
                // ClaimRewards doesn't produce shielded outputs
            }
        }

        Ok(ParsedTransaction {
            tx_hash,
            tx_type: TxType::Regular,
            shielded_outputs,
            shielded_inputs,
            unshielded_outputs: Vec::new(),
            unshielded_inputs: Vec::new(),
            raw: raw_tx.to_vec(),
        })
    }

    /// Parse a system transaction — just extract the hash.
    fn parse_system_tx(
        raw_tx: &[u8],
        _tx_idx: usize,
    ) -> Result<ParsedTransaction, MidnightError> {
        let sys_tx: midnight_ledger::structure::SystemTransaction =
            midnight_serialize::tagged_deserialize(&mut &raw_tx[..]).map_err(|e| {
                MidnightError::Decode(format!("system tx tagged_deserialize failed: {e}"))
            })?;

        let tx_hash = sys_tx.transaction_hash().0 .0;

        Ok(ParsedTransaction {
            tx_hash,
            tx_type: TxType::System,
            shielded_outputs: Vec::new(),
            shielded_inputs: Vec::new(),
            unshielded_outputs: Vec::new(),
            unshielded_inputs: Vec::new(),
            raw: raw_tx.to_vec(),
        })
    }

    /// Extract shielded outputs (with ciphertexts) from a ZswapOffer.
    fn extract_offer_outputs(
        offer: &midnight_zswap::Offer<(), InMemoryDB>,
        out: &mut Vec<ShieldedOutput>,
        idx: &mut u32,
    ) {
        for output in offer.outputs.iter_deref() {
            let ciphertexts = match &output.ciphertext {
                Some(ct) => {
                    let mut buf = Vec::new();
                    if ct.serialize(&mut buf).is_ok() {
                        vec![buf]
                    } else {
                        Vec::new()
                    }
                }
                None => Vec::new(),
            };

            out.push(ShieldedOutput {
                output_index: *idx,
                ciphertexts,
                raw_coin: Vec::new(),
                ledger_version: 8,
            });
            *idx += 1;
        }

        // Transient coins also carry ciphertexts
        for transient in offer.transient.iter_deref() {
            let ciphertexts = match &transient.ciphertext {
                Some(ct) => {
                    let mut buf = Vec::new();
                    if ct.serialize(&mut buf).is_ok() {
                        vec![buf]
                    } else {
                        Vec::new()
                    }
                }
                None => Vec::new(),
            };

            out.push(ShieldedOutput {
                output_index: *idx,
                ciphertexts,
                raw_coin: Vec::new(),
                ledger_version: 8,
            });
            *idx += 1;
        }
    }

    /// Extract shielded inputs (nullifiers) from a ZswapOffer.
    fn extract_offer_inputs(
        offer: &midnight_zswap::Offer<(), InMemoryDB>,
        out: &mut Vec<ShieldedInput>,
    ) {
        for input in offer.inputs.iter_deref() {
            out.push(ShieldedInput {
                nullifier: input.nullifier.0 .0,
            });
        }
    }
}

#[cfg(feature = "subxt-sync")]
pub use real_parser::SubxtParser;
