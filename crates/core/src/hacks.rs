use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointerHack {
    pub slot: u64,
    pub tx_idx: u32,
    pub cert_idx: u32,
    pub credential: String, // Hex string
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalOutcomeHack {
    Ratified,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalHack {
    pub id: String, // Proposal hash
    pub outcome: ProposalOutcomeHack,
    pub epoch: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Hacks {
    pub pointers: Vec<PointerHack>,
    pub proposals: Vec<ProposalHack>,
}
