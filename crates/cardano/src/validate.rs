use std::borrow::Cow;

use dolos_core::{
    ChainError, ChainPoint, Domain, Genesis, MempoolAwareUtxoStore, MempoolTx, RawData,
};

use pallas::ledger::{
    primitives::{ExUnits, NetworkId, TransactionInput},
    traverse::{MultiEraInput, MultiEraOutput, MultiEraTx},
};
use serde::{Deserialize, Serialize};
use tracing::debug;

/// A serializable representation of a single script evaluation result.
/// This mirrors `pallas::ledger::validate::phase2::tx::TxEvalResult` but adds
/// `Serialize`/`Deserialize` so we can store it as bytes in `MempoolTx`.
#[derive(Debug, Serialize, Deserialize)]
pub struct SerializableEvalResult {
    pub tag: pallas::ledger::primitives::conway::RedeemerTag,
    pub index: u32,
    pub units: ExUnits,
    pub success: bool,
    pub logs: Vec<String>,
}

impl From<pallas::ledger::validate::phase2::tx::TxEvalResult> for SerializableEvalResult {
    fn from(r: pallas::ledger::validate::phase2::tx::TxEvalResult) -> Self {
        Self {
            tag: r.tag,
            index: r.index,
            units: r.units,
            success: r.success,
            logs: r.logs,
        }
    }
}

pub type SerializableEvalReport = Vec<SerializableEvalResult>;

pub fn validate_tx<D: Domain>(
    cbor: &[u8],
    utxos: &MempoolAwareUtxoStore<D>,
    tip: Option<ChainPoint>,
    genesis: &Genesis,
) -> Result<MempoolTx, ChainError> {
    let tx = MultiEraTx::decode(cbor)?;
    let hash = tx.hash();

    let pparams = crate::load_effective_pparams::<D>(utxos.state())?;
    let pparams = crate::utils::pparams_to_pallas(&pparams);

    let network_id = match genesis.shelley.network_id.as_ref() {
        Some(network) => match network.as_str() {
            "Mainnet" => Some(NetworkId::Mainnet.into()),
            "Testnet" => Some(NetworkId::Testnet.into()),
            _ => None,
        },
        None => None,
    }
    .unwrap();

    let env = pallas::ledger::validate::utils::Environment {
        prot_params: pparams,
        prot_magic: genesis.shelley.network_magic.unwrap(),
        block_slot: tip.clone().unwrap().slot(),
        network_id,
        acnt: Some(pallas::ledger::validate::utils::AccountState::default()),
    };

    let input_refs = tx.requires().iter().map(From::from).collect();

    let utxos_matches = utxos.get_utxos(input_refs)?;

    let mut pallas_utxos = pallas::ledger::validate::utils::UTxOs::new();

    for (txoref, eracbor) in utxos_matches.iter() {
        let tx_in = TransactionInput {
            transaction_id: txoref.0,
            index: txoref.1.into(),
        };

        let input = MultiEraInput::AlonzoCompatible(<Box<Cow<'_, TransactionInput>>>::from(
            Cow::Owned(tx_in),
        ));

        let rawdata = eracbor.as_ref();

        let output = MultiEraOutput::try_from(rawdata)?;

        pallas_utxos.insert(input, output);
    }

    pallas::ledger::validate::phase1::validate_tx(
        &tx,
        0,
        &env,
        &pallas_utxos,
        &mut pallas::ledger::validate::utils::CertState::default(),
    )?;

    let raw_report = evaluate_tx::<D>(cbor, utxos)?;

    for eval in raw_report.iter() {
        if !eval.success {
            return Err(ChainError::Phase2ValidationRejected(eval.logs.clone()));
        }
    }

    debug!(
        phase1 = true,
        phase2 = true,
        redeemer_count = raw_report.len(),
        "tx validated"
    );

    let era = u16::from(tx.era());
    let payload = RawData(era, cbor.into());

    let report: SerializableEvalReport = raw_report
        .into_iter()
        .map(SerializableEvalResult::from)
        .collect();

    let report_bytes = serde_json::to_vec(&report)
        .map_err(|e| ChainError::Phase2EvaluationError(e.to_string()))?;

    let tx = MempoolTx::new(hash, payload, report_bytes);

    Ok(tx)
}

pub fn evaluate_tx<D: Domain>(
    cbor: &[u8],
    utxos: &MempoolAwareUtxoStore<D>,
) -> Result<pallas::ledger::validate::phase2::EvalReport, ChainError> {
    let tx = MultiEraTx::decode(cbor)?;

    use dolos_core::TxoRef;

    let eras = crate::eras::load_era_summary::<D>(utxos.state())?;

    let pparams = crate::load_effective_pparams::<D>(utxos.state())?;

    let pparams = crate::utils::pparams_to_pallas(&pparams);

    let slot_config = pallas::ledger::validate::phase2::script_context::SlotConfig {
        slot_length: pparams.slot_length(),
        zero_slot: eras.edge().start.slot,
        zero_time: eras.edge().start.timestamp,
    };

    let input_refs = tx.requires().iter().map(From::from).collect();

    let utxos: pallas::ledger::validate::utils::UtxoMap = utxos
        .get_utxos(input_refs)?
        .into_iter()
        .map(|(TxoRef(a, b), eracbor)| {
            let era = eracbor.version().try_into().expect("era out of range");

            (
                pallas::ledger::validate::utils::TxoRef::from((a, b)),
                pallas::ledger::validate::utils::EraCbor::from((era, eracbor.bytes().into())),
            )
        })
        .collect();

    let report = pallas::ledger::validate::phase2::evaluate_tx(&tx, &pparams, &utxos, &slot_config)
        .map_err(|e| ChainError::Phase2EvaluationError(e.to_string()))?;

    Ok(report)
}
