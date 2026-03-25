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
// UtxoDecryptor trait
// ---------------------------------------------------------------------------

/// Trial decryption of shielded UTxOs using viewing keys.
///
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
// MockDecryptor — default when midnight-crypto feature is off
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// RealDecryptor — uses midnight-transient-crypto (behind feature flag)
// ---------------------------------------------------------------------------

#[cfg(feature = "midnight-crypto")]
mod real {
    use midnight_coin_structure::coin::Info;
    use midnight_serialize::Deserializable;
    use midnight_transient_crypto::encryption::{Ciphertext, SecretKey};
    use tracing::debug;

    use super::{DecryptedCoinInfo, ShieldedUtxoState, UtxoDecryptor};

    pub struct RealDecryptor;

    impl RealDecryptor {
        fn try_decrypt_ciphertexts(
            viewing_key: &[u8; 32],
            ciphertexts: &[Vec<u8>],
        ) -> Option<Info> {
            let sk: SecretKey = {
                let ct_opt = SecretKey::from_repr(viewing_key);
                if ct_opt.is_none().into() {
                    return None;
                }
                ct_opt.unwrap()
            };

            for ct_bytes in ciphertexts {
                let ciphertext = match Ciphertext::deserialize(&mut ct_bytes.as_slice(), 0) {
                    Ok(ct) => ct,
                    Err(e) => {
                        debug!(error = %e, "failed to deserialize ciphertext, skipping");
                        continue;
                    }
                };

                if let Some(info) = sk.decrypt::<Info>(&ciphertext) {
                    return Some(info);
                }
            }

            None
        }
    }

    impl UtxoDecryptor for RealDecryptor {
        fn can_decrypt(&self, viewing_key: &[u8; 32], utxo: &ShieldedUtxoState) -> bool {
            Self::try_decrypt_ciphertexts(viewing_key, &utxo.ciphertexts).is_some()
        }

        fn decrypt_info(
            &self,
            viewing_key: &[u8; 32],
            utxo: &ShieldedUtxoState,
        ) -> Option<DecryptedCoinInfo> {
            let info = Self::try_decrypt_ciphertexts(viewing_key, &utxo.ciphertexts)?;
            Some(DecryptedCoinInfo {
                value: info.value,
                token_type: None, // ShieldedTokenType doesn't expose raw bytes easily
            })
        }
    }
}

#[cfg(feature = "midnight-crypto")]
pub use real::RealDecryptor;
