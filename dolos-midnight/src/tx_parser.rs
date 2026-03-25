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
// ExtrinsicParser trait — Front 1 implements this
// ---------------------------------------------------------------------------

/// Parses raw substrate extrinsics into structured Midnight transactions.
///
/// Front 1 provides `SubxtParser` (real implementation using subxt codegen +
/// midnight-ledger for tagged deserialization of v7/v8 transactions).
pub trait ExtrinsicParser: Send + Sync + 'static {
    fn parse_block(&self, block: &RawSubstrateBlock) -> Result<ParsedBlock, MidnightError>;
}

// ---------------------------------------------------------------------------
// MockParser — for testing
// ---------------------------------------------------------------------------

pub struct MockParser;

impl ExtrinsicParser for MockParser {
    fn parse_block(&self, block: &RawSubstrateBlock) -> Result<ParsedBlock, MidnightError> {
        // Phase 0: return empty parsed block (no transactions extracted)
        Ok(ParsedBlock {
            slot: block.number,
            hash: block.hash,
            timestamp: block.timestamp,
            transactions: Vec::new(),
        })
    }
}
