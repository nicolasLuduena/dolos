use dolos_core::hacks::{Hacks, ProposalOutcomeHack};
use pallas::ledger::primitives::StakeCredential;

pub mod pointers {
    use super::*;
    use pallas::ledger::addresses::Pointer;

    pub fn pointer_to_cred(hacks: &Hacks, pointer: &Pointer) -> Option<StakeCredential> {
        hacks
            .pointers
            .iter()
            .find(|hack| {
                hack.slot == pointer.slot()
                    && (hack.tx_idx as u64) == pointer.tx_idx()
                    && (hack.cert_idx as u64) == pointer.cert_idx()
            })
            .and_then(|hack| hack.credential.parse().ok())
            .map(StakeCredential::AddrKeyhash)
    }
}

pub mod proposals {
    use super::*;
    use pallas::ledger::primitives::Epoch;

    pub enum ProposalOutcome {
        Canceled(Epoch),
        Ratified(Epoch),
        RatifiedCurrentEpoch,
        Unknown,
    }

    pub fn outcome(hacks: &Hacks, _protocol: u16, proposal: &str) -> ProposalOutcome {
        let hack = hacks.proposals.iter().find(|p| p.id == proposal);

        if let Some(hack) = hack {
            match hack.outcome {
                ProposalOutcomeHack::Ratified => ProposalOutcome::Ratified(hack.epoch),
                ProposalOutcomeHack::Canceled => ProposalOutcome::Canceled(hack.epoch),
            }
        } else {
            ProposalOutcome::Unknown
        }
    }
}
