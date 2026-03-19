#[subxt::subxt(
    runtime_metadata_path = "../../../midnight-indexer/.node/0.22.0/metadata.scale",
    derive_for_type(
        path = "sp_consensus_slots::Slot",
        derive = "parity_scale_codec::Encode, parity_scale_codec::Decode",
        recursive
    )
)]
pub mod midnight_runtime {}
