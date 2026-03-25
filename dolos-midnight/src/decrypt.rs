use crate::model::ShieldedUtxoState;

// ---------------------------------------------------------------------------
// Decrypted coin info — result of trial decryption
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct DecryptedCoinInfo {
    /// Token value (native tDUST or other token).
    pub value: u128,
    /// Token type identifier (None = native tDUST).
    pub token_type: Option<[u8; 32]>,
}

// ---------------------------------------------------------------------------
// UtxoDecryptor trait — Front 1 implements crypto, Front 3 consumes
// ---------------------------------------------------------------------------

/// Trial decryption of shielded UTxOs using viewing keys.
///
/// Front 1 provides `RealDecryptor` using `midnight-transient-crypto::SecretKey`.
/// The API endpoints call this to filter UTxOs visible to a given viewing key.
pub trait UtxoDecryptor: Send + Sync + 'static {
    /// Check if any ciphertext in the UTxO can be decrypted with the given key.
    fn can_decrypt(&self, viewing_key: &[u8; 32], utxo: &ShieldedUtxoState) -> bool;

    /// Decrypt and return coin info if the viewing key matches.
    fn decrypt_info(
        &self,
        viewing_key: &[u8; 32],
        utxo: &ShieldedUtxoState,
    ) -> Option<DecryptedCoinInfo>;
}

// ---------------------------------------------------------------------------
// MockDecryptor — for testing
// ---------------------------------------------------------------------------

/// Mock decryptor that never matches any UTxO.
/// Front 1 replaces this with `RealDecryptor` backed by midnight-transient-crypto.
pub struct MockDecryptor;

impl UtxoDecryptor for MockDecryptor {
    fn can_decrypt(&self, _viewing_key: &[u8; 32], _utxo: &ShieldedUtxoState) -> bool {
        false
    }

    fn decrypt_info(
        &self,
        _viewing_key: &[u8; 32],
        _utxo: &ShieldedUtxoState,
    ) -> Option<DecryptedCoinInfo> {
        None
    }
}
